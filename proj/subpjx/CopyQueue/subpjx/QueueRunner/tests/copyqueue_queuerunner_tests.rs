use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use copyqueue_queuerunner::{
    new, QueueRunner, QueueRunnerCopyId, QueueRunnerCopyOutcome, QueueRunnerCopyWork,
    QueueRunnerEvent, QueueRunnerPeerScheme, QueueRunnerRunConfig, QueueRunnerTransferFailure,
    QueueRunnerTransferPhase, QueueRunnerTransferResult, QueueRunnerTransportErrorCategory,
};

#[test]
fn starts_eligible_copies_before_queue_close_and_uses_default_global_limit() {
    let subject = queue_runner();
    let events = recorded_events();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let _gate_cleanup = GateCleanup(Arc::clone(&gate));

    subject.start_run(QueueRunnerRunConfig {
        max_active_copies: None,
        max_total_tries_per_copy: 1,
        transfer_operation: {
            let gate = Arc::clone(&gate);
            Arc::new(move |_, _| {
                wait_for_gate(&gate);
                QueueRunnerTransferResult::Success
            })
        },
        event_sink: event_sink(&events),
    });

    for copy_id in 1..=10 {
        subject.enqueue_copy(copy_work(
            copy_id,
            QueueRunnerPeerScheme::File,
            QueueRunnerPeerScheme::File,
            "peer-a",
        ));
    }

    wait_for_slot_acquires(&events, 10, Duration::from_secs(2));
    let acquire_events = slot_acquires(&events);
    assert_eq!(10, acquire_events.len());
    assert!(acquire_events
        .iter()
        .all(|event| event.max_active_copies == 10));
    assert!(acquire_events
        .iter()
        .all(|event| event.active_after_event <= 10));

    open_gate(&gate);
    let result = subject.close_and_drain();
    assert_eq!(10, result.copies.len());
    assert!(result
        .copies
        .iter()
        .all(|copy| copy.outcome == QueueRunnerCopyOutcome::Succeeded));
    assert_event_counts(&events, 10, 10, 10, 10, 0, 0);
    assert_eq!(
        (1_u64..=10).map(|copy_id| (copy_id, 1_u32)).collect::<Vec<_>>(),
        sorted(copy_start_attempts(&events))
    );
    assert_eq!(
        (1_u64..=10).map(|copy_id| (copy_id, 1_u32)).collect::<Vec<_>>(),
        sorted(transfer_success_attempts(&events))
    );
}

#[test]
fn custom_copy_limit_is_global_across_schemes_and_not_lowered_per_peer() {
    let subject = queue_runner();
    let events = recorded_events();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let _gate_cleanup = GateCleanup(Arc::clone(&gate));

    subject.start_run(QueueRunnerRunConfig {
        max_active_copies: Some(4),
        max_total_tries_per_copy: 1,
        transfer_operation: {
            let gate = Arc::clone(&gate);
            Arc::new(move |_, _| {
                wait_for_gate(&gate);
                QueueRunnerTransferResult::Success
            })
        },
        event_sink: event_sink(&events),
    });

    let schemes = [
        (QueueRunnerPeerScheme::File, QueueRunnerPeerScheme::File),
        (QueueRunnerPeerScheme::File, QueueRunnerPeerScheme::Sftp),
        (QueueRunnerPeerScheme::Sftp, QueueRunnerPeerScheme::File),
        (QueueRunnerPeerScheme::Sftp, QueueRunnerPeerScheme::Sftp),
    ];

    for (index, (source_scheme, destination_scheme)) in schemes.iter().enumerate() {
        subject.enqueue_copy(copy_work(
            index as u64 + 1,
            *source_scheme,
            *destination_scheme,
            "same-peer-host-and-connection",
        ));
    }

    wait_for_slot_acquires(&events, 4, Duration::from_secs(2));
    let acquire_events = slot_acquires(&events);
    assert_eq!(4, acquire_events.len());
    assert!(acquire_events
        .iter()
        .all(|event| event.max_active_copies == 4));
    assert!(acquire_events
        .iter()
        .all(|event| event.active_after_event <= 4));

    open_gate(&gate);
    let result = subject.close_and_drain();
    assert_eq!(4, result.copies.len());
    assert!(result
        .copies
        .iter()
        .all(|copy| copy.outcome == QueueRunnerCopyOutcome::Succeeded));
    assert_event_counts(&events, 4, 4, 4, 4, 0, 0);
}

#[test]
fn global_copy_limit_holds_extra_queued_work_until_a_slot_is_released() {
    let subject = queue_runner();
    let events = recorded_events();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let _gate_cleanup = GateCleanup(Arc::clone(&gate));

    subject.start_run(QueueRunnerRunConfig {
        max_active_copies: Some(2),
        max_total_tries_per_copy: 1,
        transfer_operation: {
            let gate = Arc::clone(&gate);
            Arc::new(move |_, _| {
                wait_for_gate(&gate);
                QueueRunnerTransferResult::Success
            })
        },
        event_sink: event_sink(&events),
    });

    subject.enqueue_copy(copy_work(
        21,
        QueueRunnerPeerScheme::File,
        QueueRunnerPeerScheme::File,
        "peer-limit",
    ));
    subject.enqueue_copy(copy_work(
        22,
        QueueRunnerPeerScheme::Sftp,
        QueueRunnerPeerScheme::Sftp,
        "peer-limit",
    ));
    subject.enqueue_copy(copy_work(
        23,
        QueueRunnerPeerScheme::File,
        QueueRunnerPeerScheme::Sftp,
        "peer-limit",
    ));

    wait_for_slot_acquires(&events, 2, Duration::from_secs(2));
    assert_slot_acquire_count_stays(&events, 2, Duration::from_millis(100));

    open_gate(&gate);
    let result = subject.close_and_drain();
    assert_eq!(3, result.copies.len());
    assert!(result
        .copies
        .iter()
        .all(|copy| copy.outcome == QueueRunnerCopyOutcome::Succeeded));
    assert_event_counts(&events, 3, 3, 3, 3, 0, 0);
    assert!(slot_acquires(&events)
        .iter()
        .all(|event| event.active_after_event <= 2));
    assert_balanced_slot_events(&events);
}

#[test]
fn retryable_failure_moves_copy_behind_other_work_and_keeps_try_counts_independent() {
    let subject = queue_runner();
    let events = recorded_events();
    let attempts = Arc::new(Mutex::new(Vec::new()));

    subject.start_run(QueueRunnerRunConfig {
        max_active_copies: Some(1),
        max_total_tries_per_copy: 2,
        transfer_operation: {
            let attempts = Arc::clone(&attempts);
            Arc::new(move |copy, try_number| {
                attempts
                    .lock()
                    .expect("attempts mutex poisoned")
                    .push((copy.copy_id.value, try_number));
                if copy.copy_id.value == 1 && try_number == 1 {
                    QueueRunnerTransferResult::Failure(failure())
                } else {
                    QueueRunnerTransferResult::Success
                }
            })
        },
        event_sink: event_sink(&events),
    });

    subject.enqueue_copy(copy_work(
        1,
        QueueRunnerPeerScheme::File,
        QueueRunnerPeerScheme::File,
        "peer-a",
    ));
    subject.enqueue_copy(copy_work(
        2,
        QueueRunnerPeerScheme::File,
        QueueRunnerPeerScheme::Sftp,
        "peer-b",
    ));
    subject.enqueue_copy(copy_work(
        3,
        QueueRunnerPeerScheme::Sftp,
        QueueRunnerPeerScheme::File,
        "peer-c",
    ));

    let result = subject.close_and_drain();

    assert_eq!(
        vec![(1, 1), (2, 1), (3, 1), (1, 2)],
        attempts.lock().expect("attempts mutex poisoned").clone()
    );
    assert_copy_result(&result, 1, 2, QueueRunnerCopyOutcome::Succeeded);
    assert_copy_result(&result, 2, 1, QueueRunnerCopyOutcome::Succeeded);
    assert_copy_result(&result, 3, 1, QueueRunnerCopyOutcome::Succeeded);
    assert_event_counts(&events, 4, 4, 4, 3, 0, 1);
    assert_eq!(
        vec![(1, 1), (1, 2), (2, 1), (3, 1)],
        sorted(copy_start_attempts(&events))
    );
    assert_eq!(vec![(1, 1)], sorted(transfer_failure_attempts(&events)));
    assert_eq!(
        vec![(1, 2), (2, 1), (3, 1)],
        sorted(transfer_success_attempts(&events))
    );
    assert_balanced_slot_events(&events);
}

#[test]
fn exhausted_try_limit_stops_requeueing_the_failed_copy() {
    let subject = queue_runner();
    let events = recorded_events();
    let attempts = Arc::new(Mutex::new(Vec::new()));

    subject.start_run(QueueRunnerRunConfig {
        max_active_copies: Some(3),
        max_total_tries_per_copy: 2,
        transfer_operation: {
            let attempts = Arc::clone(&attempts);
            Arc::new(move |copy, try_number| {
                attempts
                    .lock()
                    .expect("attempts mutex poisoned")
                    .push((copy.copy_id.value, try_number));
                QueueRunnerTransferResult::Failure(failure())
            })
        },
        event_sink: event_sink(&events),
    });

    subject.enqueue_copy(copy_work(
        9,
        QueueRunnerPeerScheme::Sftp,
        QueueRunnerPeerScheme::Sftp,
        "peer-z",
    ));

    let result = subject.close_and_drain();

    assert_eq!(
        vec![(9, 1), (9, 2)],
        attempts.lock().expect("attempts mutex poisoned").clone()
    );
    assert_copy_result(&result, 9, 2, QueueRunnerCopyOutcome::FailedAfterTryLimit);
    assert_event_counts(&events, 2, 2, 2, 0, 0, 2);
    assert_eq!(vec![(9, 1), (9, 2)], sorted(copy_start_attempts(&events)));
    assert_eq!(
        vec![(9, 1), (9, 2)],
        sorted(transfer_failure_attempts(&events))
    );
    assert_balanced_slot_events(&events);
}

#[test]
fn skip_result_records_skipped_copy_and_does_not_requeue_it() {
    let subject = queue_runner();
    let events = recorded_events();
    let attempts = Arc::new(Mutex::new(Vec::new()));

    subject.start_run(QueueRunnerRunConfig {
        max_active_copies: Some(2),
        max_total_tries_per_copy: 3,
        transfer_operation: {
            let attempts = Arc::clone(&attempts);
            Arc::new(move |copy, try_number| {
                attempts
                    .lock()
                    .expect("attempts mutex poisoned")
                    .push((copy.copy_id.value, try_number));
                QueueRunnerTransferResult::SkipForRun(failure())
            })
        },
        event_sink: event_sink(&events),
    });

    subject.enqueue_copy(copy_work(
        11,
        QueueRunnerPeerScheme::Sftp,
        QueueRunnerPeerScheme::File,
        "peer-skip",
    ));

    let result = subject.close_and_drain();

    assert_eq!(
        vec![(11, 1)],
        attempts.lock().expect("attempts mutex poisoned").clone()
    );
    assert_copy_result(&result, 11, 1, QueueRunnerCopyOutcome::SkippedForRun);
    assert_event_counts(&events, 1, 1, 1, 0, 1, 0);
    assert_eq!(vec![(11, 1)], copy_start_attempts(&events));
    assert_eq!(vec![(11, 1)], transfer_skip_attempts(&events));
    assert_balanced_slot_events(&events);
}

type RecordedEvents = Arc<(Mutex<Vec<QueueRunnerEvent>>, Condvar)>;

fn queue_runner() -> Arc<dyn QueueRunner> {
    new(copyqueue_stagedtransfer::new())
}

fn recorded_events() -> RecordedEvents {
    Arc::new((Mutex::new(Vec::new()), Condvar::new()))
}

fn event_sink(events: &RecordedEvents) -> Arc<dyn Fn(QueueRunnerEvent) + Send + Sync> {
    let events = Arc::clone(events);
    Arc::new(move |event| {
        let (lock, notify) = &*events;
        lock.lock().expect("events mutex poisoned").push(event);
        notify.notify_all();
    })
}

fn wait_for_slot_acquires(events: &RecordedEvents, wanted: usize, timeout: Duration) {
    let (lock, notify) = &**events;
    let deadline = Instant::now() + timeout;
    let mut guard = lock.lock().expect("events mutex poisoned");

    while guard
        .iter()
        .filter(|event| matches!(event, QueueRunnerEvent::CopySlotAcquire(_)))
        .count()
        < wanted
    {
        let now = Instant::now();
        assert!(now < deadline, "timed out waiting for copy slot acquires");
        let wait = deadline.saturating_duration_since(now);
        let (next_guard, _) = notify
            .wait_timeout(guard, wait)
            .expect("events mutex poisoned while waiting");
        guard = next_guard;
    }
}

fn assert_slot_acquire_count_stays(events: &RecordedEvents, expected: usize, duration: Duration) {
    let (lock, notify) = &**events;
    let deadline = Instant::now() + duration;
    let mut guard = lock.lock().expect("events mutex poisoned");

    loop {
        let count = guard
            .iter()
            .filter(|event| matches!(event, QueueRunnerEvent::CopySlotAcquire(_)))
            .count();
        assert_eq!(expected, count, "copy acquired a slot while limit was full");

        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let wait = deadline.saturating_duration_since(now);
        let (next_guard, _) = notify
            .wait_timeout(guard, wait)
            .expect("events mutex poisoned while waiting");
        guard = next_guard;
    }
}

fn slot_acquires(events: &RecordedEvents) -> Vec<copyqueue_queuerunner::QueueRunnerSlotEvent> {
    let (lock, _) = &**events;
    lock.lock()
        .expect("events mutex poisoned")
        .iter()
        .filter_map(|event| match event {
            QueueRunnerEvent::CopySlotAcquire(slot) => Some(slot.clone()),
            _ => None,
        })
        .collect()
}

fn assert_event_counts(
    events: &RecordedEvents,
    copy_starts: usize,
    slot_acquires: usize,
    slot_releases: usize,
    transfer_successes: usize,
    transfer_skips: usize,
    transfer_failures: usize,
) {
    let (lock, _) = &**events;
    let events = lock.lock().expect("events mutex poisoned");
    assert_eq!(
        copy_starts,
        events
            .iter()
            .filter(|event| matches!(event, QueueRunnerEvent::CopyStart(_)))
            .count()
    );
    assert_eq!(
        slot_acquires,
        events
            .iter()
            .filter(|event| matches!(event, QueueRunnerEvent::CopySlotAcquire(_)))
            .count()
    );
    assert_eq!(
        slot_releases,
        events
            .iter()
            .filter(|event| matches!(event, QueueRunnerEvent::CopySlotRelease(_)))
            .count()
    );
    assert_eq!(
        transfer_successes,
        events
            .iter()
            .filter(|event| matches!(event, QueueRunnerEvent::TransferSuccess(_)))
            .count()
    );
    assert_eq!(
        transfer_skips,
        events
            .iter()
            .filter(|event| matches!(event, QueueRunnerEvent::TransferSkip(_)))
            .count()
    );
    assert_eq!(
        transfer_failures,
        events
            .iter()
            .filter(|event| matches!(event, QueueRunnerEvent::TransferFailure(_)))
            .count()
    );
}

fn copy_start_attempts(events: &RecordedEvents) -> Vec<(u64, u32)> {
    let (lock, _) = &**events;
    lock.lock()
        .expect("events mutex poisoned")
        .iter()
        .filter_map(|event| match event {
            QueueRunnerEvent::CopyStart(copy) => {
                Some((copy.copy_id.value, copy.try_number))
            }
            _ => None,
        })
        .collect()
}

fn transfer_success_attempts(events: &RecordedEvents) -> Vec<(u64, u32)> {
    let (lock, _) = &**events;
    lock.lock()
        .expect("events mutex poisoned")
        .iter()
        .filter_map(|event| match event {
            QueueRunnerEvent::TransferSuccess(copy) => {
                Some((copy.copy_id.value, copy.try_number))
            }
            _ => None,
        })
        .collect()
}

fn transfer_skip_attempts(events: &RecordedEvents) -> Vec<(u64, u32)> {
    let (lock, _) = &**events;
    lock.lock()
        .expect("events mutex poisoned")
        .iter()
        .filter_map(|event| match event {
            QueueRunnerEvent::TransferSkip(transfer) => {
                Some((transfer.copy.copy_id.value, transfer.copy.try_number))
            }
            _ => None,
        })
        .collect()
}

fn transfer_failure_attempts(events: &RecordedEvents) -> Vec<(u64, u32)> {
    let (lock, _) = &**events;
    lock.lock()
        .expect("events mutex poisoned")
        .iter()
        .filter_map(|event| match event {
            QueueRunnerEvent::TransferFailure(transfer) => {
                Some((transfer.copy.copy_id.value, transfer.copy.try_number))
            }
            _ => None,
        })
        .collect()
}

fn sorted(mut attempts: Vec<(u64, u32)>) -> Vec<(u64, u32)> {
    attempts.sort();
    attempts
}

fn assert_balanced_slot_events(events: &RecordedEvents) {
    let (lock, _) = &**events;
    let events = lock.lock().expect("events mutex poisoned");
    let acquires = events
        .iter()
        .filter(|event| matches!(event, QueueRunnerEvent::CopySlotAcquire(_)))
        .count();
    let releases = events
        .iter()
        .filter(|event| matches!(event, QueueRunnerEvent::CopySlotRelease(_)))
        .count();
    assert_eq!(acquires, releases);
}

fn wait_for_gate(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, notify) = &**gate;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut open = lock.lock().expect("gate mutex poisoned");
    while !*open {
        let now = Instant::now();
        assert!(now < deadline, "timed out waiting for transfer gate");
        let wait = deadline.saturating_duration_since(now);
        let (next_open, _) = notify
            .wait_timeout(open, wait)
            .expect("gate mutex poisoned while waiting");
        open = next_open;
    }
}

fn open_gate(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, notify) = &**gate;
    *lock.lock().expect("gate mutex poisoned") = true;
    notify.notify_all();
}

struct GateCleanup(Arc<(Mutex<bool>, Condvar)>);

impl Drop for GateCleanup {
    fn drop(&mut self) {
        open_gate(&self.0);
    }
}

fn copy_work(
    value: u64,
    source_scheme: QueueRunnerPeerScheme,
    destination_scheme: QueueRunnerPeerScheme,
    destination_peer_identity: &str,
) -> QueueRunnerCopyWork {
    QueueRunnerCopyWork {
        copy_id: QueueRunnerCopyId { value },
        source_scheme,
        destination_scheme,
        user_path: format!("file-{value}.txt"),
        destination_peer_identity: destination_peer_identity.to_string(),
    }
}

fn failure() -> QueueRunnerTransferFailure {
    QueueRunnerTransferFailure {
        phase: QueueRunnerTransferPhase::ReadSource,
        transport_error: Some(QueueRunnerTransportErrorCategory::IoError),
    }
}

fn assert_copy_result(
    result: &copyqueue_queuerunner::QueueRunnerRunResult,
    copy_id: u64,
    total_tries: u32,
    outcome: QueueRunnerCopyOutcome,
) {
    let copy = result
        .copies
        .iter()
        .find(|copy| copy.copy_id.value == copy_id)
        .expect("copy result missing");
    assert_eq!(total_tries, copy.total_tries);
    assert_eq!(outcome, copy.outcome);
}
