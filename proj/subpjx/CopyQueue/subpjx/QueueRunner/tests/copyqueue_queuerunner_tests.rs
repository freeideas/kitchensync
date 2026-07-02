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
    let mut open = lock.lock().expect("gate mutex poisoned");
    while !*open {
        open = notify.wait(open).expect("gate mutex poisoned while waiting");
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
