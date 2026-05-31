use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use kitchensync::{
    CopyAttemptFailure, CopyAttemptOutcome, CopyResult, CopyTask, EntryKind, EntryMeta, RelPath,
    RunConfig, TransportError, Verbosity,
};

#[derive(Clone)]
struct RecordingProgressSink {
    events: Arc<Mutex<Vec<kitchensync::ProgressEvent>>>,
}

impl Default for RecordingProgressSink {
    fn default() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl kitchensync::runtime::ProgressSink for RecordingProgressSink {
    fn publish(&self, event: kitchensync::ProgressEvent) {
        self.events
            .lock()
            .expect("progress sink poisoned")
            .push(event);
    }
}

impl RecordingProgressSink {
    fn snapshot(&self) -> Vec<kitchensync::ProgressEvent> {
        self.events.lock().expect("progress sink poisoned").clone()
    }
}

#[derive(Clone)]
struct RecordingDiagnosticSink {
    messages: Arc<Mutex<Vec<String>>>,
}

impl Default for RecordingDiagnosticSink {
    fn default() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl kitchensync::runtime::DiagnosticSink for RecordingDiagnosticSink {
    fn publish(&self, event: kitchensync::DiagnosticEvent) {
        let message = match event {
            kitchensync::DiagnosticEvent::Error { message }
            | kitchensync::DiagnosticEvent::Info { message }
            | kitchensync::DiagnosticEvent::Trace { message } => message,
        };
        self.messages
            .lock()
            .expect("diagnostic sink poisoned")
            .push(message);
    }
}

impl RecordingDiagnosticSink {
    fn snapshot(&self) -> Vec<String> {
        self.messages
            .lock()
            .expect("diagnostic sink poisoned")
            .clone()
    }
}

#[derive(Clone)]
struct ConcurrentCopyOperation {
    in_flight: Arc<AtomicUsize>,
    max_in_flight: Arc<AtomicUsize>,
    attempts: Arc<AtomicUsize>,
    sleep_per_attempt: Duration,
}

impl ConcurrentCopyOperation {
    fn new(sleep_per_attempt: Duration) -> Self {
        Self {
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_in_flight: Arc::new(AtomicUsize::new(0)),
            attempts: Arc::new(AtomicUsize::new(0)),
            sleep_per_attempt,
        }
    }

    fn max_in_flight_observed(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }

    fn attempt_count(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    fn note_in_flight(&self, value: usize) {
        let mut current = self.max_in_flight.load(Ordering::SeqCst);
        while value > current {
            match self.max_in_flight.compare_exchange(
                current,
                value,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }
}

impl kitchensync::runtime::CopyOperation for ConcurrentCopyOperation {
    fn execute_copy_attempt(
        &self,
        task: &CopyTask,
        progress: &dyn kitchensync::runtime::ProgressSink,
    ) -> CopyAttemptOutcome {
        let destination = task.destination_path.as_str().to_string();
        let basename = task
            .destination_path
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or(task.destination_path.as_str())
            .to_string();
        let total_bytes = Some(task.winning_meta.byte_size as u64);

        let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.note_in_flight(in_flight);
        self.attempts.fetch_add(1, Ordering::SeqCst);

        progress.publish(kitchensync::ProgressEvent::CopyStarted {
            destination: destination.clone(),
            basename: basename.clone(),
            total_bytes,
        });
        progress.publish(kitchensync::ProgressEvent::CopyProgress {
            destination: destination.clone(),
            basename,
            transferred_bytes: task.winning_meta.byte_size as u64,
            total_bytes,
        });

        if !self.sleep_per_attempt.is_zero() {
            thread::sleep(self.sleep_per_attempt);
        }

        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        progress.publish(kitchensync::ProgressEvent::CopyFinished {
            destination: destination.clone(),
        });

        CopyAttemptOutcome::Success(CopyResultFromTask { task }.into_copy_result())
    }
}

#[derive(Clone)]
struct RetryAwareCopyOperation {
    attempts_by_destination: Arc<Mutex<HashMap<String, usize>>>,
    failing_attempts: usize,
}

impl RetryAwareCopyOperation {
    fn new(failing_attempts: usize) -> Self {
        Self {
            attempts_by_destination: Arc::new(Mutex::new(HashMap::new())),
            failing_attempts,
        }
    }

    fn attempts_for_destination(&self, destination: &str) -> usize {
        *self
            .attempts_by_destination
            .lock()
            .expect("retry operation state poisoned")
            .get(destination)
            .unwrap_or(&0)
    }
}

impl kitchensync::runtime::CopyOperation for RetryAwareCopyOperation {
    fn execute_copy_attempt(
        &self,
        task: &CopyTask,
        _progress: &dyn kitchensync::runtime::ProgressSink,
    ) -> CopyAttemptOutcome {
        let destination = task.destination_path.as_str().to_string();
        let attempt = {
            let mut attempts = self
                .attempts_by_destination
                .lock()
                .expect("retry operation state poisoned");
            let entry = attempts.entry(destination.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        if attempt <= self.failing_attempts {
            CopyAttemptOutcome::Failure(CopyAttemptFailure {
                phase: kitchensync::TransferPhase::ReadSource,
                error: TransportError::IoError,
                message: Some(format!("attempt={attempt}")),
            })
        } else {
            CopyAttemptOutcome::Success(CopyResultFromTask { task }.into_copy_result())
        }
    }
}

#[derive(Clone)]
struct OrderAwareCopyOperation {
    attempts_by_destination: Arc<Mutex<HashMap<String, usize>>>,
    attempt_log: Arc<Mutex<Vec<String>>>,
    failing_attempts_by_destination: Arc<HashMap<String, usize>>,
    sleep_per_attempt: Duration,
}

impl OrderAwareCopyOperation {
    fn new(
        failing_attempts_by_destination: HashMap<String, usize>,
        sleep_per_attempt: Duration,
    ) -> Self {
        Self {
            attempts_by_destination: Arc::new(Mutex::new(HashMap::new())),
            attempt_log: Arc::new(Mutex::new(Vec::new())),
            failing_attempts_by_destination: Arc::new(failing_attempts_by_destination),
            sleep_per_attempt,
        }
    }

    fn attempt_log(&self) -> Vec<String> {
        self.attempt_log
            .lock()
            .expect("order operation log poisoned")
            .clone()
    }
}

impl kitchensync::runtime::CopyOperation for OrderAwareCopyOperation {
    fn execute_copy_attempt(
        &self,
        task: &CopyTask,
        _progress: &dyn kitchensync::runtime::ProgressSink,
    ) -> CopyAttemptOutcome {
        let destination = task.destination_path.as_str().to_string();

        {
            let mut log = self
                .attempt_log
                .lock()
                .expect("order operation log poisoned");
            log.push(destination.clone());
        }

        let attempt = {
            let mut attempts = self
                .attempts_by_destination
                .lock()
                .expect("order operation state poisoned");
            let entry = attempts.entry(destination.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        let failure_budget = *self
            .failing_attempts_by_destination
            .get(&destination)
            .unwrap_or(&0);

        if attempt <= failure_budget {
            CopyAttemptOutcome::Failure(CopyAttemptFailure {
                phase: kitchensync::TransferPhase::ReadSource,
                error: TransportError::IoError,
                message: Some(format!("attempt={attempt}")),
            })
        } else {
            if !self.sleep_per_attempt.is_zero() {
                thread::sleep(self.sleep_per_attempt);
            }
            CopyAttemptOutcome::Success(CopyResultFromTask { task }.into_copy_result())
        }
    }
}

struct CopyResultFromTask<'a> {
    task: &'a CopyTask,
}

impl<'a> CopyResultFromTask<'a> {
    fn into_copy_result(self) -> CopyResult {
        CopyResult {
            source_peer_id: self.task.source_peer_id,
            source_path: self.task.source_path.clone(),
            destination_peer_id: self.task.destination_peer_id,
            destination_path: self.task.destination_path.clone(),
            bytes_copied: self.task.winning_meta.byte_size as u64,
            completed: true,
            failed_phase: None,
            error: None,
        }
    }
}

fn run_config_for_tests(
    max_copies: usize,
    retries_copy: usize,
    verbosity: Verbosity,
    dry_run: bool,
) -> RunConfig {
    RunConfig {
        dry_run,
        max_copies,
        retries_copy,
        retries_list: 0,
        timeout_conn: 1,
        timeout_idle: 1,
        verbosity,
        keep_tmp_days: 0,
        keep_bak_days: 0,
        keep_del_days: 0,
        excludes: Vec::new(),
    }
}

fn mk_task(index: usize) -> CopyTask {
    CopyTask {
        source_peer_id: 1000 + index as u64,
        source_path: RelPath::new(format!("source/{index}.txt")).expect("valid source path"),
        destination_peer_id: 2000 + index as u64,
        destination_path: RelPath::new(format!("destination/{index}.txt"))
            .expect("valid destination path"),
        winning_meta: EntryMeta {
            name: format!("{index}.txt"),
            kind: EntryKind::File,
            mod_time: kitchensync::Timestamp("seed".to_string()),
            byte_size: 128,
        },
    }
}

fn parse_copy_slot_message(message: &str) -> Option<(usize, usize)> {
    let payload = message.strip_prefix("copy-slots active=")?;
    let mut parts = payload.split('/');
    let active = parts.next()?.parse().ok()?;
    let max = parts.next()?.parse().ok()?;
    Some((active, max))
}

#[test]
fn copy_scheduler_limits_inflight_attempts_to_configured_max_copies() {
    let diagnostics = RecordingDiagnosticSink::default();
    let progress = RecordingProgressSink::default();

    let config = kitchensync::runtime::SchedulerConfig {
        max_copies: 2,
        retries_copy: 1,
    };
    let operation = ConcurrentCopyOperation::new(Duration::from_millis(25));
    let scheduler =
        kitchensync::runtime::CopyScheduler::new(config, diagnostics.clone(), progress.clone());

    for index in 0..6 {
        scheduler.submit(mk_task(index));
    }
    scheduler.close();

    let summary = scheduler.run_until_complete(&operation);

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 6,
            failed: 0,
        }
    );
    assert_eq!(operation.max_in_flight_observed(), 2);
    assert_eq!(operation.attempt_count(), 6);

    let copy_slot_messages: Vec<String> = diagnostics
        .snapshot()
        .into_iter()
        .filter(|message| message.starts_with("copy-slots active="))
        .collect();

    assert!(!copy_slot_messages.is_empty());
    for message in &copy_slot_messages {
        let (active, max) = parse_copy_slot_message(message)
            .expect("copy slot trace must use the canonical format");
        assert!(active <= max);
        assert_eq!(max, 2);
    }
}

#[test]
fn copy_scheduler_retries_until_success_when_budget_allows() {
    let diagnostics = RecordingDiagnosticSink::default();
    let operation = RetryAwareCopyOperation::new(1);
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 2,
            retries_copy: 2,
        },
        diagnostics,
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(0));
    scheduler.submit(mk_task(1));
    scheduler.close();

    let summary = scheduler.run_until_complete(&operation);

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 2,
            failed: 0,
        }
    );
    assert_eq!(operation.attempts_for_destination("destination/0.txt"), 2);
    assert_eq!(operation.attempts_for_destination("destination/1.txt"), 2);
}

#[test]
fn copy_scheduler_treats_retries_copy_as_total_tries() {
    let operation = RetryAwareCopyOperation::new(usize::MAX);
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 1,
        },
        RecordingDiagnosticSink::default(),
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(0));
    scheduler.close();

    let summary = scheduler.run_until_complete(&operation);

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 0,
            failed: 1,
        }
    );
    assert_eq!(operation.attempts_for_destination("destination/0.txt"), 1);
}

#[test]
fn copy_scheduler_executes_copy_attempts_in_dry_run_mode() {
    let config = run_config_for_tests(1, 1, Verbosity::Info, true);
    let scheduler_config = kitchensync::runtime::SchedulerConfig::from_run_config(&config);
    let operation = ConcurrentCopyOperation::new(Duration::from_millis(0));
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        scheduler_config,
        RecordingDiagnosticSink::default(),
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(0));
    scheduler.close();
    let summary = scheduler.run_until_complete(&operation);

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 1,
            failed: 0,
        }
    );
    assert_eq!(operation.attempt_count(), 1);
    assert_eq!(operation.in_flight(), 0);
}

#[test]
fn copy_scheduler_retries_in_dry_run_mode_with_same_budget() {
    let config = run_config_for_tests(1, 2, Verbosity::Info, true);
    let scheduler_config = kitchensync::runtime::SchedulerConfig::from_run_config(&config);
    let operation = RetryAwareCopyOperation::new(1);
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        scheduler_config,
        RecordingDiagnosticSink::default(),
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(0));
    scheduler.close();
    let summary = scheduler.run_until_complete(&operation);

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 1,
            failed: 0,
        }
    );
    assert_eq!(operation.attempts_for_destination("destination/0.txt"), 2);
}

#[test]
fn copy_scheduler_requeues_retryable_failures_after_other_work() {
    let mut failing_attempts = HashMap::new();
    failing_attempts.insert("destination/0.txt".to_string(), 1);

    let operation = OrderAwareCopyOperation::new(failing_attempts, Duration::from_millis(0));
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 2,
        },
        RecordingDiagnosticSink::default(),
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(0));
    scheduler.submit(mk_task(1));
    scheduler.close();

    let summary = scheduler.run_until_complete(&operation);

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 2,
            failed: 0,
        }
    );
    assert_eq!(
        operation.attempt_log(),
        vec![
            "destination/0.txt".to_string(),
            "destination/1.txt".to_string(),
            "destination/0.txt".to_string(),
        ]
    );
}

// Rendering-specific behavior of stdout-based progress and diagnostic output
// (live row layout, refresh cadence, control sequences, and terminal-only output
// semantics) is verified by shell/integration tests where output channels are
// actually exercised. This module-test file intentionally focuses on runtime
// scheduling semantics that are directly observable through the public API.

#[test]
fn copy_scheduler_accepts_submissions_until_closed() {
    let operation = ConcurrentCopyOperation::new(Duration::from_millis(20));
    let diagnostics = RecordingDiagnosticSink::default();
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 1,
        },
        diagnostics,
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(0));
    let scheduler_for_run = scheduler.clone();
    let operation_for_run = operation.clone();
    let handle = thread::spawn(move || scheduler_for_run.run_until_complete(&operation_for_run));

    thread::sleep(Duration::from_millis(5));
    scheduler.submit(mk_task(1));

    thread::sleep(Duration::from_millis(5));
    scheduler.close();

    let summary = handle.join().expect("run loop should terminate");
    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 2,
            failed: 0,
        }
    );
    assert_eq!(operation.attempt_count(), 2);
}

#[test]
fn copy_scheduler_ignores_submissions_after_close() {
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 1,
        },
        RecordingDiagnosticSink::default(),
        RecordingProgressSink::default(),
    );
    let operation = ConcurrentCopyOperation::new(Duration::from_millis(0));

    scheduler.submit(mk_task(0));
    scheduler.close();
    scheduler.submit(mk_task(1));

    let summary = scheduler.run_until_complete(&operation);

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 1,
            failed: 0,
        }
    );
}

#[test]
fn copy_scheduler_emits_progress_events_for_copy_attempt_lifecycle() {
    let progress = RecordingProgressSink::default();
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 1,
        },
        RecordingDiagnosticSink::default(),
        progress.clone(),
    );

    scheduler.submit(mk_task(0));
    scheduler.close();
    let _ = scheduler.run_until_complete(&ConcurrentCopyOperation::new(Duration::from_millis(0)));

    let events = progress.snapshot();
    assert!(events
        .iter()
        .any(|event| { matches!(event, kitchensync::ProgressEvent::CopyStarted { .. }) }));
    assert!(events
        .iter()
        .any(|event| { matches!(event, kitchensync::ProgressEvent::CopyProgress { .. }) }));
    assert!(events
        .iter()
        .any(|event| { matches!(event, kitchensync::ProgressEvent::CopyFinished { .. }) }));
}

#[test]
fn copy_scheduler_emits_copy_slot_trace_events_on_acquire_and_release() {
    let diagnostics = RecordingDiagnosticSink::default();
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 1,
        },
        diagnostics.clone(),
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(0));
    scheduler.close();
    let _ = scheduler.run_until_complete(&ConcurrentCopyOperation::new(Duration::from_millis(0)));

    let messages = diagnostics.snapshot();
    let slot_messages: Vec<String> = messages
        .into_iter()
        .filter(|message| message.starts_with("copy-slots active="))
        .collect();

    assert_eq!(slot_messages.len(), 2);
    assert!(slot_messages
        .iter()
        .any(|message| message == "copy-slots active=1/1"));
    assert!(slot_messages
        .iter()
        .any(|message| message == "copy-slots active=0/1"));
}

#[test]
fn scheduler_config_extracts_copy_limits_from_run_config() {
    let run_config = run_config_for_tests(4, 3, Verbosity::Debug, false);
    let runtime_config = kitchensync::runtime::SchedulerConfig::from_run_config(&run_config);

    assert_eq!(
        runtime_config,
        kitchensync::runtime::SchedulerConfig {
            max_copies: 4,
            retries_copy: 3,
        }
    );
}

#[test]
fn copy_failure_diagnostics_include_path_peer_phase_and_error() {
    let diagnostics = RecordingDiagnosticSink::default();
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 1,
        },
        diagnostics.clone(),
        RecordingProgressSink::default(),
    );

    scheduler.submit(mk_task(1));
    scheduler.close();
    let summary = scheduler.run_until_complete(&RetryAwareCopyOperation::new(usize::MAX));

    let messages = diagnostics.snapshot();
    assert_eq!(summary.failed, 1);
    assert!(messages.iter().any(
        |message| message.contains("copy failed path=destination/1.txt")
            && message.contains("destination_peer=2001")
            && message.contains("phase=read_source")
            && message.contains("error=io_error")
    ));
}

#[test]
fn stdout_output_sinks_do_not_change_scheduler_summary() {
    let config = run_config_for_tests(1, 1, Verbosity::Error, false);
    let diagnostics = kitchensync::runtime::stdout_diagnostic_sink(
        &config,
        kitchensync::runtime::RuntimeOutputMode::LineOriented,
    );
    let progress = kitchensync::runtime::stdout_progress_sink(
        &config,
        kitchensync::runtime::RuntimeOutputMode::LineOriented,
    );
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 1,
        },
        diagnostics,
        progress,
    );

    scheduler.submit(mk_task(0));
    scheduler.close();
    let summary =
        scheduler.run_until_complete(&ConcurrentCopyOperation::new(Duration::from_millis(0)));

    assert_eq!(
        summary,
        kitchensync::runtime::SchedulerSummary {
            succeeded: 1,
            failed: 0,
        }
    );
}
