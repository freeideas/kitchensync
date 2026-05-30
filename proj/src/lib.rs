//! KitchenSync library crate.

use std::fmt;
use std::future::Future;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub config: RunConfig,
    pub peers: Vec<PeerSpec>,
    pub excludes: Vec<RelPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    pub dry_run: bool,
    pub max_copies: usize,
    pub retries_copy: usize,
    pub retries_list: usize,
    pub timeout_conn: u32,
    pub timeout_idle: u32,
    pub verbosity: Verbosity,
    pub keep_tmp_days: u32,
    pub keep_bak_days: u32,
    pub keep_del_days: u32,
    pub excludes: Vec<RelPath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub keep_tmp_days: u32,
    pub keep_bak_days: u32,
    pub keep_del_days: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Error,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSpec {
    pub role: PeerRole,
    pub urls: Vec<PeerUrl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    Canon,
    Subordinate,
    Normal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerUrl {
    pub scheme: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: String,
    pub identity: String,
    pub timeout_conn: Option<u32>,
    pub timeout_idle: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelPath(String);

impl RelPath {
    pub fn new(value: impl Into<String>) -> Result<Self, RelPathError> {
        let value = value.into();
        if !value.is_empty()
            && (value.starts_with('/')
                || value.ends_with('/')
                || value.contains('\\')
                || value.contains('\0')
                || value
                    .split('/')
                    .any(|segment| segment.is_empty() || segment == "." || segment == ".."))
        {
            return Err(RelPathError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelPathError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMeta {
    pub name: String,
    pub kind: EntryKind,
    pub mod_time: Timestamp,
    pub byte_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    NotFound,
    PermissionDenied,
    IoError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportTimeouts {
    pub timeout_conn: u32,
    pub timeout_idle: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportRootMode {
    RequireExisting,
    CreateMissing,
}

pub type PeerId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectivePeerRole {
    Canon,
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug)]
pub struct PeerSession {
    pub id: PeerId,
    pub invocation_index: usize,
    pub normalized_identity: PeerUrl,
    pub selected_url: PeerUrl,
    pub declared_role: PeerRole,
    pub effective_role: EffectivePeerRole,
    pub transport: TransportHandle,
    pub had_startup_snapshot: bool,
}

#[derive(Clone)]
pub struct TransportHandle {
    backend: Arc<dyn TransportBackend>,
}

impl fmt::Debug for TransportHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransportHandle").finish_non_exhaustive()
    }
}

impl TransportHandle {
    pub fn new(backend: impl TransportBackend + 'static) -> Self {
        Self {
            backend: Arc::new(backend),
        }
    }

    pub fn list_dir(&self, path: &RelPath) -> Result<Vec<EntryMeta>, TransportError> {
        self.backend.list_dir(path)
    }

    pub fn stat(&self, path: &RelPath) -> Result<EntryMeta, TransportError> {
        self.backend.stat(path)
    }

    pub fn open_read(&self, path: &RelPath) -> Result<TransportRead, TransportError> {
        self.backend.open_read(path)
    }

    pub fn open_write(&self, path: &RelPath) -> Result<TransportWrite, TransportError> {
        self.backend.open_write(path)
    }

    pub fn rename_no_overwrite(&self, src: &RelPath, dst: &RelPath) -> Result<(), TransportError> {
        self.backend.rename_no_overwrite(src, dst)
    }

    pub fn delete_file(&self, path: &RelPath) -> Result<(), TransportError> {
        self.backend.delete_file(path)
    }

    pub fn create_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        self.backend.create_dir(path)
    }

    pub fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        self.backend.delete_dir(path)
    }

    pub fn set_mod_time(&self, path: &RelPath, time: Timestamp) -> Result<(), TransportError> {
        self.backend.set_mod_time(path, time)
    }
}

pub trait TransportBackend: Send + Sync {
    fn list_dir(&self, path: &RelPath) -> Result<Vec<EntryMeta>, TransportError>;
    fn stat(&self, path: &RelPath) -> Result<EntryMeta, TransportError>;
    fn open_read(&self, path: &RelPath) -> Result<TransportRead, TransportError>;
    fn open_write(&self, path: &RelPath) -> Result<TransportWrite, TransportError>;
    fn rename_no_overwrite(&self, src: &RelPath, dst: &RelPath) -> Result<(), TransportError>;
    fn delete_file(&self, path: &RelPath) -> Result<(), TransportError>;
    fn create_dir(&self, path: &RelPath) -> Result<(), TransportError>;
    fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError>;
    fn set_mod_time(&self, path: &RelPath, time: Timestamp) -> Result<(), TransportError>;
}

pub struct TransportRead {
    inner: Box<dyn Read + Send>,
}

impl fmt::Debug for TransportRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransportRead").finish_non_exhaustive()
    }
}

impl TransportRead {
    pub fn new(inner: impl Read + Send + 'static) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }
}

impl Read for TransportRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

pub struct TransportWrite {
    inner: Box<dyn Write + Send>,
    close: Option<Box<dyn FnOnce(Box<dyn Write + Send>) -> Result<(), TransportError> + Send>>,
}

impl TransportWrite {
    pub fn new(inner: impl Write + Send + 'static) -> Self {
        Self {
            inner: Box::new(inner),
            close: Some(Box::new(|mut inner| {
                inner.flush().map_err(|_| TransportError::IoError)
            })),
        }
    }

    pub fn with_close(
        inner: impl Write + Send + 'static,
        close: impl FnOnce(Box<dyn Write + Send>) -> Result<(), TransportError> + Send + 'static,
    ) -> Self {
        Self {
            inner: Box::new(inner),
            close: Some(Box::new(close)),
        }
    }

    pub fn close(mut self) -> Result<(), TransportError> {
        let close = self.close.take();
        let inner = std::mem::replace(&mut self.inner, Box::new(std::io::sink()));
        match close {
            Some(close) => close(inner),
            None => Ok(()),
        }
    }
}

impl Write for TransportWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPhase {
    ReadSource,
    WriteSwapNew,
    MoveExistingToSwapOld,
    RenameFinal,
    SetModTime,
    ArchiveOld,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyResult {
    pub source_peer_id: PeerId,
    pub source_path: RelPath,
    pub destination_peer_id: PeerId,
    pub destination_path: RelPath,
    pub bytes_copied: u64,
    pub completed: bool,
    pub failed_phase: Option<TransferPhase>,
    pub error: Option<TransportError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyTask {
    pub source_peer_id: PeerId,
    pub source_path: RelPath,
    pub destination_peer_id: PeerId,
    pub destination_path: RelPath,
    pub winning_meta: EntryMeta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticEvent {
    Error { message: String },
    Info { message: String },
    Trace { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    Scanning {
        directory: RelPath,
    },
    CopyStarted {
        destination: String,
        basename: String,
        total_bytes: Option<u64>,
    },
    CopyProgress {
        destination: String,
        basename: String,
        transferred_bytes: u64,
        total_bytes: Option<u64>,
    },
    CopyFinished {
        destination: String,
    },
    CopyRemoved {
        destination: String,
    },
}

pub trait DiagnosticSink: Send + Sync {
    fn publish(&self, event: DiagnosticEvent);
}

pub trait ProgressSink: Send + Sync {
    fn publish(&self, event: ProgressEvent);
}

pub fn absolute_path_from(base: &std::path::Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

pub trait TransportFactory: Send + Sync {
    fn connect(
        &self,
        url: &PeerUrl,
        timeouts: TransportTimeouts,
        root_mode: TransportRootMode,
    ) -> Result<TransportHandle, TransportError>;
}

#[path = "../cli/mod.rs"]
pub mod cli;

#[path = "../operations/mod.rs"]
pub mod operations;

#[path = "../peer/mod.rs"]
pub mod peer;

#[path = "../runtime/mod.rs"]
pub mod runtime;

pub use runtime::{CopyAttemptFailure, CopyAttemptOutcome};

#[path = "../snapshot/mod.rs"]
pub mod snapshot;

#[path = "../sync/mod.rs"]
pub mod sync;

#[path = "../transport/mod.rs"]
pub mod transport;

impl DiagnosticSink for runtime::StdoutDiagnosticSink {
    fn publish(&self, event: DiagnosticEvent) {
        runtime::DiagnosticSink::publish(self, event);
    }
}

impl ProgressSink for runtime::StdoutProgressSink {
    fn publish(&self, event: ProgressEvent) {
        runtime::ProgressSink::publish(self, event);
    }
}

pub fn module_summaries() -> Vec<&'static str> {
    vec![
        cli::summary(),
        operations::summary(),
        peer::summary(),
        runtime::summary(),
        snapshot::summary(),
        sync::summary(),
        transport::summary(),
    ]
}

pub fn run_process<I, S>(args: I, env: &cli::CliParseEnv) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
{
    match cli::parse_invocation(args, env) {
        cli::CliInvocation::Help { help } => {
            print!("{help}");
            0
        }
        cli::CliInvocation::Invalid { error, help } => {
            println!("{}", error.message);
            print!("{help}");
            1
        }
        cli::CliInvocation::Run(request) => run_request(request),
    }
}

fn run_request(request: RunRequest) -> i32 {
    let output_mode = runtime::RuntimeOutputMode::LineOriented;
    let diagnostics = runtime::stdout_diagnostic_sink(&request.config, output_mode);
    let progress = runtime::stdout_progress_sink(&request.config, output_mode);
    let scheduler_progress = runtime::stdout_progress_sink(&request.config, output_mode);
    let transport_factory = transport::factory();

    let pending = match block_on(peer::connect_peers(
        &request.config,
        &request.peers,
        &transport_factory,
        &diagnostics,
    )) {
        Ok(pending) => pending,
        Err(error) => {
            diagnostics.publish(DiagnosticEvent::Error {
                message: error.message().to_string(),
            });
            return 1;
        }
    };

    let tmp_root = std::env::temp_dir().join("kitchensync");
    let snapshot_mode = if request.config.dry_run {
        snapshot::SnapshotStartupMode::DryRun
    } else {
        snapshot::SnapshotStartupMode::Normal
    };

    let mut retained_pending = Vec::new();
    let mut stores = Vec::new();
    let mut existence = Vec::new();

    for pending_session in pending {
        let startup_session = PeerSession {
            id: pending_session.id,
            invocation_index: pending_session.invocation_index,
            normalized_identity: pending_session.normalized_identity.clone(),
            selected_url: pending_session.selected_url.clone(),
            declared_role: pending_session.declared_role,
            effective_role: EffectivePeerRole::Subordinate,
            transport: pending_session.transport.clone(),
            had_startup_snapshot: false,
        };

        match snapshot::prepare_peer_snapshot(&startup_session, &tmp_root, snapshot_mode) {
            Ok(open) => {
                existence.push(peer::SnapshotExistence {
                    peer_id: pending_session.id,
                    existed: open.had_history_at_startup,
                });
                retained_pending.push(pending_session);
                stores.push(open.store);
            }
            Err(error) => {
                diagnostics.publish(DiagnosticEvent::Error {
                    message: format_snapshot_error(&error),
                });
            }
        }
    }

    if retained_pending.len() < 2 {
        diagnostics.publish(DiagnosticEvent::Error {
            message: peer::PeerStartupError::TooFewReachablePeers
                .message()
                .to_string(),
        });
        return 1;
    }

    let sessions = match peer::resolve_roles(retained_pending, &existence) {
        Ok(sessions) => sessions,
        Err(error) => {
            diagnostics.publish(DiagnosticEvent::Error {
                message: error.message().to_string(),
            });
            return 1;
        }
    };

    let operations = operations::executor(&request.config, &diagnostics, &progress);
    let scheduler = runtime::CopyScheduler::new(
        runtime::SchedulerConfig::from_run_config(&request.config),
        diagnostics.clone(),
        scheduler_progress,
    );

    let report = {
        let mut sync_peers = sessions
            .iter()
            .zip(stores.iter_mut())
            .map(|(session, snapshot)| sync::SyncPeer { session, snapshot })
            .collect::<Vec<_>>();

        sync::run(sync::SyncRun {
            config: &request.config,
            peers: &mut sync_peers,
            operations: &operations,
            copy_scheduler: &scheduler,
            diagnostics: &diagnostics,
            progress: &progress,
        })
    };

    let mut failed = !report.completed;
    if !request.config.dry_run {
        for store in stores {
            let peer_id = store.peer();
            let changed = store.had_changes();
            let closed = match store.close() {
                Ok(closed) => closed,
                Err(error) => {
                    diagnostics.publish(DiagnosticEvent::Error {
                        message: format_snapshot_error(&error),
                    });
                    failed = true;
                    continue;
                }
            };

            if !changed {
                continue;
            }

            if let Some(session) = sessions.iter().find(|session| session.id == peer_id) {
                if let Err(error) = snapshot::upload_peer_snapshot(session, closed) {
                    diagnostics.publish(DiagnosticEvent::Error {
                        message: format_snapshot_error(&error),
                    });
                    failed = true;
                }
            }
        }
    }

    if request.config.dry_run {
        diagnostics.publish(DiagnosticEvent::Info {
            message: "dry run: no peer-side mutations were written".to_string(),
        });
    }

    if failed {
        1
    } else {
        diagnostics.publish(DiagnosticEvent::Info {
            message: "KitchenSync completed successfully".to_string(),
        });
        0
    }
}

fn format_snapshot_error(error: &snapshot::SnapshotError) -> String {
    match error {
        snapshot::SnapshotError::Transport {
            peer,
            category,
            operation,
        } => format!(
            "Snapshot transport failure for peer {peer}: {:?} during {:?}",
            category, operation
        ),
        snapshot::SnapshotError::InvalidDatabase { peer, reason } => {
            format!("Invalid snapshot database for peer {peer}: {:?}", reason)
        }
        snapshot::SnapshotError::LocalIo { peer, operation } => {
            format!(
                "Local snapshot I/O failure for peer {peer}: {:?}",
                operation
            )
        }
    }
}

fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    struct NoopWaker;

    impl Wake for NoopWaker {
        fn wake(self: Arc<Self>) {}
    }

    let waker = Waker::from(Arc::new(NoopWaker));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);

    loop {
        match Future::poll(future.as_mut(), &mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
