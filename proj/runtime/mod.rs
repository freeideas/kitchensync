use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub use crate::{DiagnosticEvent, ProgressEvent};

use crate::{
    snapshot::fresh_timestamp, CopyResult, CopyTask, RunConfig, TransferPhase, TransportError,
    Verbosity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerConfig {
    pub max_copies: usize,
    pub retries_copy: usize,
}

impl SchedulerConfig {
    pub fn from_run_config(config: &RunConfig) -> Self {
        Self {
            max_copies: config.max_copies,
            retries_copy: config.retries_copy,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SchedulerSummary {
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyAttemptFailure {
    pub phase: TransferPhase,
    pub error: TransportError,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyAttemptOutcome {
    Success(CopyResult),
    Failure(CopyAttemptFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOutputMode {
    Interactive,
    LineOriented,
}

pub trait CopyOperation: Send + Sync {
    fn execute_copy_attempt(
        &self,
        task: &CopyTask,
        progress: &dyn ProgressSink,
    ) -> CopyAttemptOutcome;
}

pub trait ProgressSink: Send + Sync {
    fn publish(&self, event: ProgressEvent);
}

pub trait DiagnosticSink: Send + Sync {
    fn publish(&self, event: DiagnosticEvent);
}

pub struct CopyScheduler {
    config: SchedulerConfig,
    shared: Arc<SchedulerShared>,
    diagnostics: Arc<dyn DiagnosticSink>,
    progress: Arc<dyn ProgressSink>,
}

impl Clone for CopyScheduler {
    fn clone(&self) -> Self {
        Self {
            config: self.config,
            shared: Arc::clone(&self.shared),
            diagnostics: Arc::clone(&self.diagnostics),
            progress: Arc::clone(&self.progress),
        }
    }
}

impl CopyScheduler {
    pub fn new(
        config: SchedulerConfig,
        diagnostics: impl DiagnosticSink + Send + Sync + 'static,
        progress: impl ProgressSink + Send + Sync + 'static,
    ) -> Self {
        Self {
            config,
            shared: Arc::new(SchedulerShared::default()),
            diagnostics: Arc::new(diagnostics),
            progress: Arc::new(progress),
        }
    }

    pub fn submit(&self, task: CopyTask) {
        let mut state = self.shared.state.lock().expect("copy scheduler poisoned");
        if state.closed {
            return;
        }
        state.queue.push_back(QueuedCopyTask { task, tries: 0 });
        self.shared.available.notify_one();
    }

    pub fn close(&self) {
        let mut state = self.shared.state.lock().expect("copy scheduler poisoned");
        state.closed = true;
        self.shared.available.notify_all();
    }

    pub fn run_until_complete(&self, operation: &dyn CopyOperation) -> SchedulerSummary {
        thread::scope(|scope| {
            for _ in 0..self.config.max_copies {
                let shared = Arc::clone(&self.shared);
                let diagnostics = Arc::clone(&self.diagnostics);
                let progress = Arc::clone(&self.progress);
                let config = self.config;

                scope.spawn(move || {
                    worker_loop(&shared, config, operation, &*diagnostics, &*progress);
                });
            }
        });

        let state = self.shared.state.lock().expect("copy scheduler poisoned");
        state.summary
    }
}

#[derive(Default)]
struct SchedulerShared {
    state: Mutex<SchedulerState>,
    available: Condvar,
}

#[derive(Default)]
struct SchedulerState {
    queue: VecDeque<QueuedCopyTask>,
    closed: bool,
    active: usize,
    summary: SchedulerSummary,
}

struct QueuedCopyTask {
    task: CopyTask,
    tries: usize,
}

fn worker_loop(
    shared: &SchedulerShared,
    config: SchedulerConfig,
    operation: &dyn CopyOperation,
    diagnostics: &dyn DiagnosticSink,
    progress: &dyn ProgressSink,
) {
    loop {
        let mut queued = {
            let mut state = shared.state.lock().expect("copy scheduler poisoned");
            loop {
                if state.active < config.max_copies {
                    if let Some(task) = state.queue.pop_front() {
                        state.active += 1;
                        publish_copy_slot_trace(diagnostics, state.active, config.max_copies);
                        break task;
                    }
                }

                if state.closed && state.active == 0 {
                    shared.available.notify_all();
                    return;
                }

                if config.max_copies == 0 && state.closed {
                    return;
                }

                state = shared
                    .available
                    .wait(state)
                    .expect("copy scheduler poisoned");
            }
        };

        publish_copy_started(progress, &queued.task);
        let outcome = operation.execute_copy_attempt(&queued.task, progress);
        let progress_task = queued.task.clone();

        let finished_successfully = matches!(outcome, CopyAttemptOutcome::Success(_));
        {
            let mut state = shared.state.lock().expect("copy scheduler poisoned");
            state.active -= 1;
            publish_copy_slot_trace(diagnostics, state.active, config.max_copies);

            match outcome {
                CopyAttemptOutcome::Success(_) => {
                    state.summary.succeeded += 1;
                }
                CopyAttemptOutcome::Failure(failure) => {
                    queued.tries += 1;
                    publish_copy_failure(diagnostics, &queued.task, &failure);

                    if queued.tries < config.retries_copy {
                        state.queue.push_back(queued);
                    } else {
                        state.summary.failed += 1;
                    }
                }
            }

            shared.available.notify_all();
        }

        if finished_successfully {
            publish_copy_finished(progress, &progress_task);
        }
        publish_copy_removed(progress, &progress_task);
    }
}

fn publish_copy_slot_trace(diagnostics: &dyn DiagnosticSink, active: usize, max: usize) {
    diagnostics.publish(DiagnosticEvent::Trace {
        message: format!("copy-slots active={active}/{max}"),
    });
}

fn publish_copy_failure(
    diagnostics: &dyn DiagnosticSink,
    task: &CopyTask,
    failure: &CopyAttemptFailure,
) {
    let mut message = format!(
        "copy failed path={} destination_peer={} phase={} error={}",
        render_rel_path(&task.destination_path),
        task.destination_peer_id,
        TransferPhaseDisplay(failure.phase),
        TransportErrorDisplay(&failure.error)
    );

    if let Some(detail) = &failure.message {
        if !detail.is_empty() {
            message.push_str(": ");
            message.push_str(detail);
        }
    }

    diagnostics.publish(DiagnosticEvent::Error { message });
}

fn publish_copy_started(progress: &dyn ProgressSink, task: &CopyTask) {
    progress.publish(ProgressEvent::CopyStarted {
        destination: copy_destination_key(task),
        basename: copy_basename(task).to_string(),
        total_bytes: copy_total_bytes(task),
    });
}

fn publish_copy_finished(progress: &dyn ProgressSink, task: &CopyTask) {
    progress.publish(ProgressEvent::CopyFinished {
        destination: copy_destination_key(task),
    });
}

fn publish_copy_removed(progress: &dyn ProgressSink, task: &CopyTask) {
    progress.publish(ProgressEvent::CopyRemoved {
        destination: copy_destination_key(task),
    });
}

fn copy_destination_key(task: &CopyTask) -> String {
    format!(
        "{}:{}",
        task.destination_peer_id,
        render_rel_path(&task.destination_path)
    )
}

fn copy_basename(task: &CopyTask) -> &str {
    task.destination_path
        .as_str()
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(&task.winning_meta.name)
}

fn copy_total_bytes(task: &CopyTask) -> Option<u64> {
    u64::try_from(task.winning_meta.byte_size).ok()
}

pub fn stdout_diagnostic_sink(
    config: &RunConfig,
    _mode: RuntimeOutputMode,
) -> StdoutDiagnosticSink {
    StdoutDiagnosticSink {
        verbosity: config.verbosity,
        stdout: Arc::new(Mutex::new(())),
    }
}

pub fn stdout_progress_sink(config: &RunConfig, mode: RuntimeOutputMode) -> StdoutProgressSink {
    StdoutProgressSink {
        verbosity: config.verbosity,
        mode,
        state: Mutex::new(ProgressRenderState::new(config.max_copies.max(1))),
    }
}

#[derive(Clone)]
pub struct StdoutDiagnosticSink {
    verbosity: Verbosity,
    stdout: Arc<Mutex<()>>,
}

impl StdoutDiagnosticSink {
    pub fn publish(&self, event: DiagnosticEvent) {
        DiagnosticSink::publish(self, event);
    }
}

impl DiagnosticSink for StdoutDiagnosticSink {
    fn publish(&self, event: DiagnosticEvent) {
        match event {
            DiagnosticEvent::Error { message } => {
                self.write_line(&message);
            }
            DiagnosticEvent::Trace { message } if self.verbosity == Verbosity::Trace => {
                self.write_line(&format!("{} {message}", fresh_timestamp().0));
            }
            DiagnosticEvent::Info { message }
                if matches!(
                    self.verbosity,
                    Verbosity::Info | Verbosity::Debug | Verbosity::Trace
                ) =>
            {
                self.write_line(&message);
            }
            _ => {}
        }
    }
}

impl StdoutDiagnosticSink {
    fn write_line(&self, message: &str) {
        let _guard = self.stdout.lock().expect("stdout diagnostic sink poisoned");
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{message}");
        let _ = stdout.flush();
    }
}

pub struct StdoutProgressSink {
    verbosity: Verbosity,
    mode: RuntimeOutputMode,
    state: Mutex<ProgressRenderState>,
}

impl StdoutProgressSink {
    pub fn publish(&self, event: ProgressEvent) {
        ProgressSink::publish(self, event);
    }
}

impl ProgressSink for StdoutProgressSink {
    fn publish(&self, event: ProgressEvent) {
        if self.verbosity == Verbosity::Error {
            return;
        }

        let mut state = self.state.lock().expect("stdout progress sink poisoned");
        let render_priority = state.apply(event);

        if !state.should_render() {
            if render_priority == RenderPriority::WhenAllowed {
                state.wait_until_render_allowed();
            } else {
                return;
            }
        }

        state.mark_rendered();
        match self.mode {
            RuntimeOutputMode::Interactive => render_interactive_progress(&state),
            RuntimeOutputMode::LineOriented => render_line_progress(&state),
        }
        state.remove_rendered_finished_rows();
    }
}

struct ProgressRenderState {
    max_visible_copies: usize,
    current_scan: Option<String>,
    copy_rows: Vec<CopyProgressRow>,
    last_rendered: Option<Instant>,
}

impl ProgressRenderState {
    fn new(max_visible_copies: usize) -> Self {
        Self {
            max_visible_copies,
            current_scan: None,
            copy_rows: Vec::new(),
            last_rendered: None,
        }
    }

    fn apply(&mut self, event: ProgressEvent) -> RenderPriority {
        match event {
            ProgressEvent::Scanning { directory } => {
                self.current_scan = Some(render_scan_path(&directory));
            }
            ProgressEvent::CopyStarted {
                destination,
                basename,
                total_bytes,
            } => {
                self.upsert_copy_row(destination, basename, 0, total_bytes);
            }
            ProgressEvent::CopyProgress {
                destination,
                basename,
                transferred_bytes,
                total_bytes,
            } => {
                self.upsert_copy_row(destination, basename, transferred_bytes, total_bytes);
            }
            ProgressEvent::CopyFinished { destination } => {
                if let Some(row) = self
                    .copy_rows
                    .iter_mut()
                    .find(|row| row.destination == destination)
                {
                    row.transferred_bytes = row.total_bytes.unwrap_or(row.transferred_bytes);
                    row.finished = true;
                    return RenderPriority::WhenAllowed;
                }
            }
            ProgressEvent::CopyRemoved { destination } => {
                if let Some(row) = self
                    .copy_rows
                    .iter_mut()
                    .find(|row| row.destination == destination)
                {
                    if row.finished && !row.finished_rendered {
                        row.remove_after_render = true;
                    } else {
                        row.remove_now = true;
                    }
                }
            }
        }
        RenderPriority::Normal
    }

    fn upsert_copy_row(
        &mut self,
        destination: String,
        basename: String,
        transferred_bytes: u64,
        total_bytes: Option<u64>,
    ) {
        if let Some(row) = self
            .copy_rows
            .iter_mut()
            .find(|row| row.destination == destination)
        {
            row.basename = basename;
            row.transferred_bytes = transferred_bytes;
            row.total_bytes = total_bytes;
            row.finished = false;
            row.finished_rendered = false;
            row.remove_after_render = false;
            row.remove_now = false;
            return;
        }

        self.copy_rows.push(CopyProgressRow {
            destination,
            basename,
            transferred_bytes,
            total_bytes,
            finished: false,
            finished_rendered: false,
            remove_after_render: false,
            remove_now: false,
        });
    }

    fn should_render(&self) -> bool {
        match self.last_rendered {
            None => true,
            Some(last) => last.elapsed() >= Duration::from_secs(1),
        }
    }

    fn mark_rendered(&mut self) {
        self.last_rendered = Some(Instant::now());
        for row in &mut self.copy_rows {
            if row.finished {
                row.finished_rendered = true;
            }
        }
    }

    fn wait_until_render_allowed(&self) {
        if let Some(last) = self.last_rendered {
            let elapsed = last.elapsed();
            if elapsed < Duration::from_secs(1) {
                thread::sleep(Duration::from_secs(1) - elapsed);
            }
        }
    }

    fn visible_rows(&self) -> impl Iterator<Item = &CopyProgressRow> {
        self.copy_rows
            .iter()
            .filter(|row| !row.remove_now)
            .take(self.max_visible_copies)
    }

    fn remove_rendered_finished_rows(&mut self) {
        self.copy_rows
            .retain(|row| !row.remove_now && !(row.remove_after_render && row.finished_rendered));
    }
}

struct CopyProgressRow {
    destination: String,
    basename: String,
    transferred_bytes: u64,
    total_bytes: Option<u64>,
    finished: bool,
    finished_rendered: bool,
    remove_after_render: bool,
    remove_now: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderPriority {
    Normal,
    WhenAllowed,
}

fn render_interactive_progress(state: &ProgressRenderState) {
    let mut stdout = io::stdout().lock();
    let _ = write!(stdout, "\x1b[2J\x1b[H");

    for row in state.visible_rows() {
        let _ = writeln!(stdout, "{} {}", row.basename, progress_bar(row));
    }

    let _ = writeln!(
        stdout,
        "Scanning: {}",
        state.current_scan.as_deref().unwrap_or(".")
    );
    let _ = stdout.flush();
}

fn render_line_progress(state: &ProgressRenderState) {
    let mut stdout = io::stdout().lock();
    for row in state.visible_rows() {
        let _ = writeln!(stdout, "{} {}", row.basename, progress_bar(row));
    }
    let _ = writeln!(
        stdout,
        "Scanning: {}",
        state.current_scan.as_deref().unwrap_or(".")
    );
    let _ = stdout.flush();
}

fn progress_bar(row: &CopyProgressRow) -> String {
    const WIDTH: u64 = 20;

    match row.total_bytes {
        Some(total) if total > 0 => {
            let filled = ((row.transferred_bytes.min(total) * WIDTH) / total) as usize;
            let empty = WIDTH as usize - filled;
            format!(
                "[{}{}] {}/{}",
                "#".repeat(filled),
                ".".repeat(empty),
                row.transferred_bytes.min(total),
                total
            )
        }
        Some(_) => "[####################] 0/0".to_string(),
        None => format!("{} bytes", row.transferred_bytes),
    }
}

fn render_scan_path(path: &crate::RelPath) -> String {
    render_rel_path(path)
}

fn render_rel_path(path: &crate::RelPath) -> String {
    let rendered = path.as_str();
    if rendered.is_empty() {
        ".".to_string()
    } else {
        rendered.to_string()
    }
}

struct TransferPhaseDisplay(TransferPhase);

impl fmt::Display for TransferPhaseDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            TransferPhase::ReadSource => "read_source",
            TransferPhase::WriteSwapNew => "write_swap_new",
            TransferPhase::MoveExistingToSwapOld => "move_existing_to_swap_old",
            TransferPhase::RenameFinal => "rename_final",
            TransferPhase::SetModTime => "set_mod_time",
            TransferPhase::ArchiveOld => "archive_old",
            TransferPhase::Cleanup => "cleanup",
        })
    }
}

struct TransportErrorDisplay<'a>(&'a TransportError);

impl fmt::Display for TransportErrorDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            TransportError::NotFound => "not_found",
            TransportError::PermissionDenied => "permission_denied",
            TransportError::IoError => "io_error",
        })
    }
}

pub fn summary() -> &'static str {
    "runtime: copy scheduling, retries, progress output, trace logging, and error aggregation."
}
