use std::sync::Arc;
use std::collections::VecDeque;
use crate::api::*;

struct QueueRunnerImpl {
    _stagedtransfer: std::sync::Arc<dyn copyqueue_stagedtransfer::StagedTransfer>,
    shared: Arc<SharedRunState>,
}

struct SharedRunState {
    state: std::sync::Mutex<Option<RunState>>,
    changed: std::sync::Condvar,
}

struct RunState {
    config: QueueRunnerRunConfig,
    queue: VecDeque<QueuedCopy>,
    active_copies: u32,
    closed: bool,
    results: Vec<QueueRunnerCopyResult>,
}

#[derive(Clone)]
struct QueuedCopy {
    work: QueueRunnerCopyWork,
    completed_tries: u32,
}

impl QueueRunner for QueueRunnerImpl {
    fn start_run(&self, config: QueueRunnerRunConfig) {
        let mut state = self.shared.state.lock().expect("queue runner mutex poisoned");
        *state = Some(RunState {
            config,
            queue: VecDeque::new(),
            active_copies: 0,
            closed: false,
            results: Vec::new(),
        });
    }

    fn enqueue_copy(&self, copy: QueueRunnerCopyWork) {
        {
            let mut state = self.shared.state.lock().expect("queue runner mutex poisoned");
            let run = state.as_mut().expect("queue runner run was not started");
            run.queue.push_back(QueuedCopy {
                work: copy,
                completed_tries: 0,
            });
        }

        schedule_available_work(Arc::clone(&self.shared));
    }

    fn close_and_drain(&self) -> QueueRunnerRunResult {
        {
            let mut state = self.shared.state.lock().expect("queue runner mutex poisoned");
            let run = state.as_mut().expect("queue runner run was not started");
            run.closed = true;
        }

        schedule_available_work(Arc::clone(&self.shared));

        let mut state = self.shared.state.lock().expect("queue runner mutex poisoned");
        loop {
            let run = state.as_ref().expect("queue runner run was not started");
            if run.closed && run.queue.is_empty() && run.active_copies == 0 {
                break;
            }
            state = self
                .shared
                .changed
                .wait(state)
                .expect("queue runner mutex poisoned while waiting");
        }

        let run = state.take().expect("queue runner run was not started");
        QueueRunnerRunResult {
            copies: run.results,
        }
    }
}

pub fn new(stagedtransfer: std::sync::Arc<dyn copyqueue_stagedtransfer::StagedTransfer>) -> std::sync::Arc<dyn QueueRunner> {
    Arc::new(QueueRunnerImpl {
        _stagedtransfer: stagedtransfer,
        shared: Arc::new(SharedRunState {
            state: std::sync::Mutex::new(None),
            changed: std::sync::Condvar::new(),
        }),
    })
}

fn schedule_available_work(shared: Arc<SharedRunState>) {
    loop {
        let scheduled = {
            let mut state = shared.state.lock().expect("queue runner mutex poisoned");
            let run = state.as_mut().expect("queue runner run was not started");
            let max_active_copies = run.config.max_active_copies.unwrap_or(10);

            if run.active_copies >= max_active_copies {
                None
            } else {
                run.queue.pop_front().map(|queued| {
                    let try_number = queued.completed_tries + 1;
                    let attempt = QueueRunnerCopyAttempt {
                        copy_id: queued.work.copy_id,
                        user_path: queued.work.user_path.clone(),
                        destination_peer_identity: queued.work.destination_peer_identity.clone(),
                        try_number,
                    };

                    run.active_copies += 1;
                    let active_after_acquire = run.active_copies;
                    let transfer_operation = Arc::clone(&run.config.transfer_operation);
                    let event_sink = Arc::clone(&run.config.event_sink);
                    let max_total_tries = run.config.max_total_tries_per_copy;

                    event_sink(QueueRunnerEvent::CopyStart(attempt.clone()));
                    event_sink(QueueRunnerEvent::CopySlotAcquire(QueueRunnerSlotEvent {
                        copy: attempt.clone(),
                        active_after_event: active_after_acquire,
                        max_active_copies,
                    }));

                    ScheduledCopy {
                        queued,
                        attempt,
                        transfer_operation,
                        event_sink,
                        max_active_copies,
                        max_total_tries,
                    }
                })
            }
        };

        let Some(scheduled) = scheduled else {
            return;
        };

        let thread_shared = Arc::clone(&shared);
        std::thread::spawn(move || run_scheduled_copy(thread_shared, scheduled));
    }
}

struct ScheduledCopy {
    queued: QueuedCopy,
    attempt: QueueRunnerCopyAttempt,
    transfer_operation: QueueRunnerTransferOperation,
    event_sink: QueueRunnerEventSink,
    max_active_copies: u32,
    max_total_tries: u32,
}

fn run_scheduled_copy(shared: Arc<SharedRunState>, scheduled: ScheduledCopy) {
    let result = (scheduled.transfer_operation)(
        scheduled.queued.work.clone(),
        scheduled.attempt.try_number,
    );

    {
        let mut state = shared.state.lock().expect("queue runner mutex poisoned");
        let run = state.as_mut().expect("queue runner run was not started");

        run.active_copies -= 1;
        let active_after_release = run.active_copies;

        (scheduled.event_sink)(QueueRunnerEvent::CopySlotRelease(QueueRunnerSlotEvent {
            copy: scheduled.attempt.clone(),
            active_after_event: active_after_release,
            max_active_copies: scheduled.max_active_copies,
        }));

        match result {
            QueueRunnerTransferResult::Success => {
                (scheduled.event_sink)(QueueRunnerEvent::TransferSuccess(
                    scheduled.attempt.clone(),
                ));
                run.results.push(copy_result(
                    &scheduled.queued.work,
                    scheduled.attempt.try_number,
                    QueueRunnerCopyOutcome::Succeeded,
                ));
            }
            QueueRunnerTransferResult::SkipForRun(failure) => {
                (scheduled.event_sink)(QueueRunnerEvent::TransferSkip(QueueRunnerTransferEvent {
                        copy: scheduled.attempt.clone(),
                        failure,
                    }));
                run.results.push(copy_result(
                    &scheduled.queued.work,
                    scheduled.attempt.try_number,
                    QueueRunnerCopyOutcome::SkippedForRun,
                ));
            }
            QueueRunnerTransferResult::Failure(failure) => {
                (scheduled.event_sink)(QueueRunnerEvent::TransferFailure(QueueRunnerTransferEvent {
                        copy: scheduled.attempt.clone(),
                        failure,
                    }));

                if scheduled.attempt.try_number < scheduled.max_total_tries {
                    run.queue.push_back(QueuedCopy {
                        work: scheduled.queued.work,
                        completed_tries: scheduled.attempt.try_number,
                    });
                } else {
                    run.results.push(copy_result(
                        &scheduled.queued.work,
                        scheduled.attempt.try_number,
                        QueueRunnerCopyOutcome::FailedAfterTryLimit,
                    ));
                }
            }
        }

        shared.changed.notify_all();
    }

    schedule_available_work(shared);
}

fn copy_result(
    work: &QueueRunnerCopyWork,
    total_tries: u32,
    outcome: QueueRunnerCopyOutcome,
) -> QueueRunnerCopyResult {
    QueueRunnerCopyResult {
        copy_id: work.copy_id,
        user_path: work.user_path.clone(),
        destination_peer_identity: work.destination_peer_identity.clone(),
        total_tries,
        outcome,
    }
}
