use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Default, Clone)]
struct NullSink;

impl kitchensync::runtime::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::runtime::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::ProgressEvent) {}
}

impl kitchensync::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::ProgressEvent) {}
}

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

#[derive(Clone)]
#[allow(dead_code)]
struct DirectoryCreationOrderExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    create_calls: Arc<Mutex<Vec<String>>>,
}

impl<'a> kitchensync::operations::OperationExecutor for DirectoryCreationOrderExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        self.delegate.recover_directory_swaps(peer, directory)
    }

    fn displace_to_bak(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
        timestamp: kitchensync::Timestamp,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DisplacementReport> {
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.create_calls
            .lock()
            .expect("create directory call log poisoned")
            .push(path.as_str().to_string());
        self.delegate.create_directory(peer, path)
    }

    fn cleanup_retention(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
        now: kitchensync::Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::CleanupReport> {
        self.delegate
            .cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)
    }

    fn execute_copy_attempt(
        &self,
        source_peer: &kitchensync::PeerSession,
        source_path: &kitchensync::RelPath,
        destination_peer: &kitchensync::PeerSession,
        destination_path: &kitchensync::RelPath,
        winning_meta: &kitchensync::EntryMeta,
    ) -> kitchensync::CopyResult {
        self.delegate.execute_copy_attempt(
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

#[derive(Clone)]
struct OperationCallCounter<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    recover_directory_swaps_calls: Arc<AtomicUsize>,
    cleanup_retention_calls: Arc<AtomicUsize>,
}

impl<'a> OperationCallCounter<'a> {
    fn recover_directory_swaps_calls(&self) -> usize {
        self.recover_directory_swaps_calls.load(Ordering::SeqCst)
    }

    fn cleanup_retention_calls(&self) -> usize {
        self.cleanup_retention_calls.load(Ordering::SeqCst)
    }
}

impl<'a> kitchensync::operations::OperationExecutor for OperationCallCounter<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        self.recover_directory_swaps_calls
            .fetch_add(1, Ordering::SeqCst);
        self.delegate.recover_directory_swaps(peer, directory)
    }

    fn displace_to_bak(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
        timestamp: kitchensync::Timestamp,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DisplacementReport> {
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.delegate.create_directory(peer, path)
    }

    fn cleanup_retention(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
        now: kitchensync::Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::CleanupReport> {
        self.cleanup_retention_calls.fetch_add(1, Ordering::SeqCst);
        self.delegate
            .cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)
    }

    fn execute_copy_attempt(
        &self,
        source_peer: &kitchensync::PeerSession,
        source_path: &kitchensync::RelPath,
        destination_peer: &kitchensync::PeerSession,
        destination_path: &kitchensync::RelPath,
        winning_meta: &kitchensync::EntryMeta,
    ) -> kitchensync::CopyResult {
        self.delegate.execute_copy_attempt(
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

#[derive(Clone)]
struct FailingCreateDirectoryExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_create_directories: Vec<String>,
}

impl<'a> kitchensync::operations::OperationExecutor for FailingCreateDirectoryExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        self.delegate.recover_directory_swaps(peer, directory)
    }

    fn displace_to_bak(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
        timestamp: kitchensync::Timestamp,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DisplacementReport> {
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        if self
            .fail_create_directories
            .iter()
            .any(|target| target == path.as_str())
        {
            return Err(kitchensync::operations::OperationError {
                peer_id: peer.id,
                context: kitchensync::operations::OperationErrorContext::CreateDirectory {
                    path: path.clone(),
                },
                error: kitchensync::TransportError::PermissionDenied,
            });
        }

        self.delegate.create_directory(peer, path)
    }

    fn cleanup_retention(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
        now: kitchensync::Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::CleanupReport> {
        self.delegate
            .cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)
    }

    fn execute_copy_attempt(
        &self,
        source_peer: &kitchensync::PeerSession,
        source_path: &kitchensync::RelPath,
        destination_peer: &kitchensync::PeerSession,
        destination_path: &kitchensync::RelPath,
        winning_meta: &kitchensync::EntryMeta,
    ) -> kitchensync::CopyResult {
        self.delegate.execute_copy_attempt(
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

#[derive(Clone)]
struct ListingFailureBackend {
    attempts: Arc<AtomicUsize>,
    fail_attempts: usize,
}

impl ListingFailureBackend {
    fn new(fail_attempts: usize) -> Self {
        Self {
            attempts: Arc::new(AtomicUsize::new(0)),
            fail_attempts,
        }
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }
}

impl kitchensync::TransportBackend for ListingFailureBackend {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        if path.as_str().is_empty() {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt <= self.fail_attempts {
                return Err(kitchensync::TransportError::IoError);
            }
            return Ok(Vec::new());
        }

        Err(kitchensync::TransportError::NotFound)
    }

    fn stat(
        &self,
        _path: &kitchensync::RelPath,
    ) -> Result<kitchensync::EntryMeta, kitchensync::TransportError> {
        Err(kitchensync::TransportError::NotFound)
    }

    fn open_read(
        &self,
        _path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportRead, kitchensync::TransportError> {
        Err(kitchensync::TransportError::NotFound)
    }

    fn open_write(
        &self,
        _path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportWrite, kitchensync::TransportError> {
        Err(kitchensync::TransportError::PermissionDenied)
    }

    fn rename_no_overwrite(
        &self,
        _source: &kitchensync::RelPath,
        _destination: &kitchensync::RelPath,
    ) -> Result<(), kitchensync::TransportError> {
        Err(kitchensync::TransportError::PermissionDenied)
    }

    fn delete_file(&self, _path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        Err(kitchensync::TransportError::PermissionDenied)
    }

    fn create_dir(&self, _path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        Err(kitchensync::TransportError::PermissionDenied)
    }

    fn delete_dir(&self, _path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        Err(kitchensync::TransportError::PermissionDenied)
    }

    fn set_mod_time(
        &self,
        _path: &kitchensync::RelPath,
        _time: kitchensync::Timestamp,
    ) -> Result<(), kitchensync::TransportError> {
        Err(kitchensync::TransportError::PermissionDenied)
    }
}

impl kitchensync::ProgressSink for RecordingProgressSink {
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

fn normalize_progress_directory(path: &kitchensync::RelPath) -> String {
    if path.as_str().is_empty() {
        ".".to_string()
    } else {
        path.as_str().to_string()
    }
}

#[derive(Clone)]
pub struct FailingRecoverOperationExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_recover_directories: Vec<String>,
}

impl<'a> kitchensync::operations::OperationExecutor for FailingRecoverOperationExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        if self
            .fail_recover_directories
            .iter()
            .any(|target| target == directory.as_str())
        {
            return Err(kitchensync::operations::OperationError {
                peer_id: peer.id,
                context: kitchensync::operations::OperationErrorContext::RecoverDirectorySwaps {
                    directory: directory.clone(),
                },
                error: kitchensync::TransportError::IoError,
            });
        }

        self.delegate.recover_directory_swaps(peer, directory)
    }

    fn displace_to_bak(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
        timestamp: kitchensync::Timestamp,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DisplacementReport> {
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.delegate.create_directory(peer, path)
    }

    fn cleanup_retention(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
        now: kitchensync::Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::CleanupReport> {
        self.delegate
            .cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)
    }

    fn execute_copy_attempt(
        &self,
        source_peer: &kitchensync::PeerSession,
        source_path: &kitchensync::RelPath,
        destination_peer: &kitchensync::PeerSession,
        destination_path: &kitchensync::RelPath,
        winning_meta: &kitchensync::EntryMeta,
    ) -> kitchensync::CopyResult {
        self.delegate.execute_copy_attempt(
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

#[derive(Clone)]
struct FailingRecoverOperationExecutorByPeer<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_recover_for_peer_and_directory: Vec<(kitchensync::PeerId, String)>,
}

impl<'a> kitchensync::operations::OperationExecutor for FailingRecoverOperationExecutorByPeer<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        if self
            .fail_recover_for_peer_and_directory
            .iter()
            .any(|(peer_id, target)| *peer_id == peer.id && target == directory.as_str())
        {
            return Err(kitchensync::operations::OperationError {
                peer_id: peer.id,
                context: kitchensync::operations::OperationErrorContext::RecoverDirectorySwaps {
                    directory: directory.clone(),
                },
                error: kitchensync::TransportError::IoError,
            });
        }

        self.delegate.recover_directory_swaps(peer, directory)
    }

    fn displace_to_bak(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
        timestamp: kitchensync::Timestamp,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DisplacementReport> {
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.delegate.create_directory(peer, path)
    }

    fn cleanup_retention(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
        now: kitchensync::Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::CleanupReport> {
        self.delegate
            .cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)
    }

    fn execute_copy_attempt(
        &self,
        source_peer: &kitchensync::PeerSession,
        source_path: &kitchensync::RelPath,
        destination_peer: &kitchensync::PeerSession,
        destination_path: &kitchensync::RelPath,
        winning_meta: &kitchensync::EntryMeta,
    ) -> kitchensync::CopyResult {
        self.delegate.execute_copy_attempt(
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    let normalized = test_name.replace(['\\', '/'], "_");
    path.push(format!("kitchensync-sync-test-{normalized}-{seq}"));

    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }

    fs::create_dir_all(&path).unwrap();
    path
}

fn write_file(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn make_peer_url(root: &Path) -> kitchensync::PeerUrl {
    let path = root.to_string_lossy().to_string();
    kitchensync::PeerUrl {
        scheme: "file".to_string(),
        username: None,
        password: None,
        host: None,
        port: None,
        path,
        identity: format!("file://{}", root.to_string_lossy()),
        timeout_conn: None,
        timeout_idle: None,
    }
}

fn make_peer_session(
    id: kitchensync::PeerId,
    root: &Path,
    role: kitchensync::EffectivePeerRole,
) -> kitchensync::PeerSession {
    let selected_url = make_peer_url(root);
    let normalized_identity = make_peer_url(root);
    let transport = kitchensync::transport::factory()
        .connect(
            &selected_url,
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();

    let declared_role = match role {
        kitchensync::EffectivePeerRole::Canon => kitchensync::PeerRole::Canon,
        kitchensync::EffectivePeerRole::Contributing => kitchensync::PeerRole::Normal,
        kitchensync::EffectivePeerRole::Subordinate => kitchensync::PeerRole::Subordinate,
    };

    kitchensync::PeerSession {
        id,
        invocation_index: 0,
        normalized_identity,
        selected_url,
        declared_role,
        effective_role: role,
        transport,
        had_startup_snapshot: false,
    }
}

fn make_peer_session_with_transport<B: kitchensync::TransportBackend + Send + Sync + 'static>(
    id: kitchensync::PeerId,
    root: &Path,
    role: kitchensync::EffectivePeerRole,
    backend: B,
) -> kitchensync::PeerSession {
    let selected_url = make_peer_url(root);
    let normalized_identity = make_peer_url(root);
    let transport = kitchensync::TransportHandle::new(backend);

    let declared_role = match role {
        kitchensync::EffectivePeerRole::Canon => kitchensync::PeerRole::Canon,
        kitchensync::EffectivePeerRole::Contributing => kitchensync::PeerRole::Normal,
        kitchensync::EffectivePeerRole::Subordinate => kitchensync::PeerRole::Subordinate,
    };

    kitchensync::PeerSession {
        id,
        invocation_index: 0,
        normalized_identity,
        selected_url,
        declared_role,
        effective_role: role,
        transport,
        had_startup_snapshot: false,
    }
}

fn make_run_config(dry_run: bool, retries_list: usize) -> kitchensync::RunConfig {
    kitchensync::RunConfig {
        dry_run,
        max_copies: 2,
        retries_copy: 2,
        retries_list,
        timeout_conn: 1,
        timeout_idle: 1,
        verbosity: kitchensync::Verbosity::Error,
        keep_tmp_days: 1,
        keep_bak_days: 1,
        keep_del_days: 1,
        excludes: Vec::new(),
    }
}

fn prepare_snapshot(
    peer: &kitchensync::PeerSession,
    test_name: &str,
) -> kitchensync::snapshot::SnapshotOpen {
    kitchensync::snapshot::prepare_peer_snapshot(
        peer,
        &next_test_root(test_name),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap()
}

fn make_scheduler() -> kitchensync::runtime::CopyScheduler {
    kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 2,
            retries_copy: 2,
        },
        NullSink,
        NullSink,
    )
}

#[test]
fn sync_run_summary_contains_contract_terms() {
    let summary = kitchensync::sync::summary().to_ascii_lowercase();

    assert!(summary.starts_with("sync:"));
    assert!(summary.contains("traversal"));
    assert!(summary.contains("reconciliation"));
    assert!(summary.contains("copy"));
    assert!(summary.contains("snapshot"));
}

#[test]
fn sync_run_excludes_built_in_system_directories() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("exclude_system_source");
    let destination_root = next_test_root("exclude_system_destination");

    let source = make_peer_session(
        91_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "exclude_system_source_db");
    let destination = make_peer_session(
        91_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "exclude_system_destination_db");

    write_file(&source_root, "keep.txt", "keep this");
    write_file(&source_root, ".git/config", "local git config");
    write_file(&source_root, ".kitchensync/manifest.toml", "state");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    let keep = kitchensync::RelPath::new("keep.txt").unwrap();
    let excluded_git = kitchensync::RelPath::new(".git/config").unwrap();
    let excluded_sync_dir = kitchensync::RelPath::new(".kitchensync/manifest.toml").unwrap();

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);
    assert!(destination_root.join("keep.txt").exists());
    assert!(!destination_root.join(".git").exists());
    assert!(!destination_root.join(".kitchensync/manifest.toml").exists());

    assert!(source_snapshot.store.lookup(&keep).unwrap().is_some());
    assert!(source_snapshot
        .store
        .lookup(&excluded_git)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&excluded_sync_dir)
        .unwrap()
        .is_none());
    assert!(destination_snapshot.store.lookup(&keep).unwrap().is_some());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_git)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_sync_dir)
        .unwrap()
        .is_none());
}

#[test]
fn sync_run_ignores_snapshot_only_rows_when_building_candidates() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("snapshot_only_source");
    let destination_root = next_test_root("snapshot_only_destination");

    let source = make_peer_session(
        92_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "snapshot_only_source_db");
    let destination = make_peer_session(
        92_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "snapshot_only_destination_db");

    let stale_path = kitchensync::RelPath::new("ghost-entry.txt").unwrap();
    let stale_meta = kitchensync::EntryMeta {
        name: "ghost-entry.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        byte_size: 5,
    };
    source_snapshot
        .store
        .upsert_confirmed_present(&stale_path, &stale_meta)
        .unwrap();
    let before = source_snapshot.store.lookup(&stale_path).unwrap().unwrap();

    assert!(destination_snapshot
        .store
        .lookup(&stale_path)
        .unwrap()
        .is_none());

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    let after = source_snapshot.store.lookup(&stale_path).unwrap().unwrap();

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 0);
    assert_eq!(report.traversal.scanned_directories, 1);
    assert_eq!(after, before);
    assert!(destination_snapshot
        .store
        .lookup(&stale_path)
        .unwrap()
        .is_none());
}

#[test]
fn sync_run_processes_candidate_entries_in_case_insensitive_order() {
    let config = make_run_config(false, 1);
    let progress = RecordingProgressSink::default();
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 1,
            retries_copy: 2,
        },
        NullSink,
        progress.clone(),
    );

    let source_root = next_test_root("case_order_source");
    let destination_root = next_test_root("case_order_destination");

    let source = make_peer_session(
        93_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "case_order_source_db");
    let destination = make_peer_session(
        93_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "case_order_destination_db");

    write_file(&source_root, "Beta.txt", "1");
    write_file(&source_root, "alpha.txt", "2");
    write_file(&source_root, "Gamma.txt", "3");
    write_file(&source_root, "delta.txt", "4");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &progress,
    });

    let starts = progress
        .snapshot()
        .into_iter()
        .filter_map(|event| match event {
            kitchensync::ProgressEvent::CopyStarted { basename, .. } => Some(basename),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 4);
    assert_eq!(
        starts,
        vec![
            "alpha.txt".to_string(),
            "Beta.txt".to_string(),
            "delta.txt".to_string(),
            "Gamma.txt".to_string()
        ]
    );
    assert!(destination_root.join("alpha.txt").exists());
    assert!(destination_root.join("Beta.txt").exists());
    assert!(destination_root.join("delta.txt").exists());
    assert!(destination_root.join("Gamma.txt").exists());
}

#[test]
fn sync_run_non_canon_peer_listing_failure_does_not_block_other_subtree_work() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingRecoverOperationExecutorByPeer {
        delegate: &base_executor,
        fail_recover_for_peer_and_directory: vec![(94_002, "shared".to_string())],
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("non_canon_failure_source");
    let destination_root = next_test_root("non_canon_failure_destination");

    let source = make_peer_session(
        94_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "non_canon_failure_source_db");
    let destination = make_peer_session(
        94_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "non_canon_failure_destination_db");

    write_file(&source_root, "root.txt", "root stay");
    write_file(&source_root, "shared/peer.txt", "only source");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(destination_root.join("root.txt").exists());
    assert!(destination_root.join("shared").exists());
    assert!(!destination_root.join("shared/peer.txt").exists());
    assert!(
        report
            .failures
            .iter()
            .any(|failure| matches!(failure, kitchensync::sync::SyncFailure::SwapRecovery { peer_id: 94_002, directory, attempts: 1, canon: false, .. } if directory.as_str() == "shared"))
    );
}

#[test]
fn sync_run_retries_root_listings_to_configured_limit() {
    let config = make_run_config(false, 3);
    let listing_transport = ListingFailureBackend::new(3);
    let source = make_peer_session_with_transport(
        95_001,
        &next_test_root("listing_retry_root_source"),
        kitchensync::EffectivePeerRole::Contributing,
        listing_transport.clone(),
    );
    let mut source_snapshot = prepare_snapshot(&source, "listing_retry_root_source_db");
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let mut peers = [kitchensync::sync::SyncPeer {
        session: &source,
        snapshot: &mut source_snapshot.store,
    }];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    let attempts = report
        .failures
        .iter()
        .find_map(|failure| match failure {
            kitchensync::sync::SyncFailure::Listing {
                peer_id,
                directory,
                attempts,
                ..
            } if *peer_id == 95_001 && directory.as_str() == "" => Some(*attempts),
            _ => None,
        })
        .expect("expected listing failure for root");

    assert!(!report.completed);
    assert_eq!(attempts, 3);
    assert_eq!(listing_transport.attempts(), 3);
}

#[test]
fn sync_run_dry_run_skips_recovery_and_cleanup_operation_calls() {
    let config = make_run_config(true, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = OperationCallCounter {
        delegate: &base_executor,
        recover_directory_swaps_calls: Arc::new(AtomicUsize::new(0)),
        cleanup_retention_calls: Arc::new(AtomicUsize::new(0)),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("dry_run_skip_ops_source");
    let destination_root = next_test_root("dry_run_skip_ops_destination");

    let source = make_peer_session(
        96_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "dry_run_skip_ops_source_db");
    let destination = make_peer_session(
        96_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "dry_run_skip_ops_destination_db");

    write_file(&source_root, "nested/payload.bin", "dry");

    let path = kitchensync::RelPath::new("nested/payload.bin").unwrap();
    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.copies.succeeded, 1);
    assert_eq!(executor.recover_directory_swaps_calls(), 0);
    assert_eq!(executor.cleanup_retention_calls(), 0);
    assert!(!destination_root.join("nested/payload.bin").exists());
    assert!(source_snapshot.store.lookup(&path).unwrap().is_some());
    assert!(destination_snapshot.store.lookup(&path).unwrap().is_some());
}

#[test]
fn sync_run_performs_cleanup_retention_in_normal_mode() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = OperationCallCounter {
        delegate: &base_executor,
        recover_directory_swaps_calls: Arc::new(AtomicUsize::new(0)),
        cleanup_retention_calls: Arc::new(AtomicUsize::new(0)),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("cleanup_normal_mode_source");
    let destination_root = next_test_root("cleanup_normal_mode_destination");

    let source = make_peer_session(
        96_101,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "cleanup_normal_mode_source_db");
    let destination = make_peer_session(
        96_102,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "cleanup_normal_mode_destination_db");

    write_file(&source_root, "root.txt", "cleanup me");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(executor.cleanup_retention_calls(), 2);
    assert!(destination_root.join("root.txt").exists());
}

#[test]
fn sync_run_marks_confirmed_absence_as_deleted_without_advancing_last_seen() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("absence_mark_source");
    let destination_root = next_test_root("absence_mark_destination");
    let subordinate_root = next_test_root("absence_mark_subordinate");

    let source = make_peer_session(
        96_201,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "absence_mark_source_db");
    let destination = make_peer_session(
        96_202,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "absence_mark_destination_db");
    let subordinate = make_peer_session(
        96_203,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot = prepare_snapshot(&subordinate, "absence_mark_subordinate_db");

    write_file(&subordinate_root, "ghost.txt", "subordinate-only");

    let path = kitchensync::RelPath::new("ghost.txt").unwrap();
    let stale_meta = kitchensync::EntryMeta {
        name: "ghost.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        byte_size: 4,
    };
    destination_snapshot
        .store
        .upsert_confirmed_present(&path, &stale_meta)
        .unwrap();
    let before = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    let before_last_seen = before.last_seen.clone();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &subordinate,
            snapshot: &mut subordinate_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert!(!destination_root.join("ghost.txt").exists());
    assert!(!subordinate_root.join("ghost.txt").exists());

    let after = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(
        after.kind,
        kitchensync::snapshot::SnapshotEntryKind::Tombstone
    );
    assert_eq!(after.last_seen, before_last_seen);
    assert_eq!(after.deleted_time, before_last_seen);
}

#[test]
fn sync_run_failed_directory_creation_keeps_snapshot_row_unchanged() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingCreateDirectoryExecutor {
        delegate: &base_executor,
        fail_create_directories: vec!["shared".to_string()],
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("failed_create_source");
    let destination_root = next_test_root("failed_create_destination");

    let source = make_peer_session(
        97_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "failed_create_source_db");
    let destination = make_peer_session(
        97_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "failed_create_destination_db");

    write_file(&source_root, "shared/payload.txt", "payload");

    let shared_path = kitchensync::RelPath::new("shared").unwrap();
    let shared_meta = kitchensync::EntryMeta {
        name: "shared".to_string(),
        kind: kitchensync::EntryKind::Directory,
        mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        byte_size: -1,
    };
    destination_snapshot
        .store
        .upsert_confirmed_present(&shared_path, &shared_meta)
        .unwrap();
    let before = destination_snapshot
        .store
        .lookup(&shared_path)
        .unwrap()
        .unwrap();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| matches!(failure, kitchensync::sync::SyncFailure::Operation { path, .. } if path.as_str() == "shared"))
    );
    assert_eq!(
        destination_snapshot
            .store
            .lookup(&shared_path)
            .unwrap()
            .unwrap(),
        before
    );
    assert!(!destination_root.join("shared").exists());
    assert!(source_root.join("shared").exists());
}

#[test]
fn sync_run_rejects_empty_peer_set() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();
    let mut peers: Vec<kitchensync::sync::SyncPeer> = Vec::new();

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert_eq!(report.traversal.scanned_directories, 0);
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        kitchensync::sync::SyncFailure::InvalidRunInput {
            reason: kitchensync::sync::SyncInputError::EmptyPeerSet
        }
    )));
}

#[test]
fn sync_run_rejects_snapshot_peer_mismatch() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("snapshot_mismatch_source");
    let wrong_root = next_test_root("snapshot_mismatch_wrong");

    let source = make_peer_session(
        10_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "snapshot_mismatch_source_db");
    let wrong = make_peer_session(
        10_002,
        &wrong_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut wrong_snapshot = prepare_snapshot(&wrong, "snapshot_mismatch_wrong_db");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &wrong,
            snapshot: &mut wrong_snapshot.store,
        },
    ];

    // Intentionally couple session 10_001 with a snapshot that belongs to 10_002.
    let (first_peer, second_peer) = peers.split_at_mut(1);
    std::mem::swap(&mut first_peer[0].snapshot, &mut second_peer[0].snapshot);

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        kitchensync::sync::SyncFailure::InvalidRunInput {
            reason: kitchensync::sync::SyncInputError::SnapshotPeerMismatch { peer_id: 10_001 },
        }
    )));
}

#[test]
fn sync_run_rejects_no_contributing_peer() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();
    let root = next_test_root("no_contributing_peer");

    let peer = make_peer_session(20_001, &root, kitchensync::EffectivePeerRole::Subordinate);
    let mut snapshot = prepare_snapshot(&peer, "no_contributing_peer_db");

    let mut peers = [kitchensync::sync::SyncPeer {
        session: &peer,
        snapshot: &mut snapshot.store,
    }];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        kitchensync::sync::SyncFailure::InvalidRunInput {
            reason: kitchensync::sync::SyncInputError::NoContributingPeer,
        }
    )));
}

#[test]
fn sync_run_rejects_multiple_canon_peers() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let canon_a_root = next_test_root("multiple_canon_a");
    let canon_b_root = next_test_root("multiple_canon_b");

    let canon_a = make_peer_session(30_001, &canon_a_root, kitchensync::EffectivePeerRole::Canon);
    let mut snapshot_a = prepare_snapshot(&canon_a, "multiple_canon_a_db");

    let canon_b = make_peer_session(30_002, &canon_b_root, kitchensync::EffectivePeerRole::Canon);
    let mut snapshot_b = prepare_snapshot(&canon_b, "multiple_canon_b_db");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &canon_a,
            snapshot: &mut snapshot_a.store,
        },
        kitchensync::sync::SyncPeer {
            session: &canon_b,
            snapshot: &mut snapshot_b.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        kitchensync::sync::SyncFailure::InvalidRunInput {
            reason: kitchensync::sync::SyncInputError::MoreThanOneCanonPeer,
        }
    )));
}

#[test]
fn sync_run_reports_root_progress_as_dot() {
    let mut config = make_run_config(false, 1);
    config.excludes.clear();

    let progress = RecordingProgressSink::default();
    let executor = kitchensync::operations::executor(&config, &NullSink, &progress);
    let scheduler = make_scheduler();

    let source_root = next_test_root("progress_root_source");
    let destination_root = next_test_root("progress_root_destination");

    let source = make_peer_session(
        40_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "progress_root_source_db");
    let destination = make_peer_session(
        40_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "progress_root_dest_db");

    write_file(&source_root, "keep.txt", "stable");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &progress,
    });

    assert!(report.completed);
    let events = progress.snapshot();
    let first_scan = events.iter().find_map(|event| match event {
        kitchensync::ProgressEvent::Scanning { directory } => Some(directory.clone()),
        _ => None,
    });

    assert_eq!(
        normalize_progress_directory(&first_scan.unwrap()),
        ".".to_string()
    );
}

#[test]
fn sync_run_excludes_file_and_directory_paths() {
    let config = kitchensync::RunConfig {
        excludes: vec![
            kitchensync::RelPath::new("cache").unwrap(),
            kitchensync::RelPath::new("notes.tmp").unwrap(),
        ],
        ..make_run_config(false, 1)
    };

    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("exclude_source");
    let destination_root = next_test_root("exclude_destination");

    let source = make_peer_session(
        50_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "exclude_source_db");
    let destination = make_peer_session(
        50_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "exclude_dest_db");

    write_file(&source_root, "notes.tmp", "skip this");
    write_file(&source_root, "cache/old.log", "skip cache");
    write_file(&source_root, "kept.txt", "keep this");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);

    let keep = kitchensync::RelPath::new("kept.txt").unwrap();
    let excluded_file = kitchensync::RelPath::new("notes.tmp").unwrap();
    let excluded_dir = kitchensync::RelPath::new("cache/old.log").unwrap();

    assert!(destination_root.join("kept.txt").exists());
    assert!(!destination_root.join("notes.tmp").exists());
    assert!(!destination_root.join("cache").exists());

    assert!(source_snapshot.store.lookup(&keep).unwrap().is_some());
    assert!(source_snapshot
        .store
        .lookup(&excluded_file)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_dir)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_file)
        .unwrap()
        .is_none());
}

#[test]
fn sync_run_prefers_file_when_file_and_directory_conflict_has_no_canon() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let file_root = next_test_root("conflict_file_root");
    let dir_root = next_test_root("conflict_dir_root");

    let file_peer = make_peer_session(
        60_001,
        &file_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut file_snapshot = prepare_snapshot(&file_peer, "conflict_file_root_db");
    let dir_peer = make_peer_session(
        60_002,
        &dir_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut dir_snapshot = prepare_snapshot(&dir_peer, "conflict_dir_root_db");

    write_file(&file_root, "entry", "winner-data");
    fs::create_dir_all(dir_root.join("entry")).unwrap();
    write_file(&dir_root, "entry/old.txt", "displaced");

    let shared_path = kitchensync::RelPath::new("entry").unwrap();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &file_peer,
            snapshot: &mut file_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &dir_peer,
            snapshot: &mut dir_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);
    assert_eq!(report.copies.succeeded, 1);

    assert!(dir_root.join("entry").is_file());
    assert!(!dir_root.join("entry").is_dir());
    assert_eq!(
        fs::read_to_string(dir_root.join("entry")).unwrap(),
        "winner-data"
    );
    assert!(file_root.join("entry").is_file());
    assert!(file_snapshot.store.lookup(&shared_path).unwrap().is_some());
}

#[test]
fn sync_run_obeys_canon_authority_when_canon_is_absent() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let canon_root = next_test_root("canon_authority_canon_root");
    let contributor_root = next_test_root("canon_authority_contributor_root");

    let canon = make_peer_session(61_001, &canon_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "canon_authority_canon_db");
    let contributor = make_peer_session(
        61_002,
        &contributor_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut contributor_snapshot = prepare_snapshot(&contributor, "canon_authority_contributor_db");

    write_file(
        &contributor_root,
        "ghost.txt",
        "present only on contributor",
    );

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &canon,
            snapshot: &mut canon_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &contributor,
            snapshot: &mut contributor_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);
    assert_eq!(report.copies.succeeded, 0);
    assert!(!contributor_root.join("ghost.txt").exists());
    assert!(!canon_root.join("ghost.txt").exists());
}

#[test]
fn sync_run_subordinate_peers_do_not_vote_but_are_reconciled_to_outcome() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("subordinate_decision_source");
    let destination_root = next_test_root("subordinate_decision_destination");
    let subordinate_root = next_test_root("subordinate_decision_subordinate");

    let source = make_peer_session(
        62_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "subordinate_decision_source_db");
    let destination = make_peer_session(
        62_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "subordinate_decision_destination_db");
    let subordinate = make_peer_session(
        62_003,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot =
        prepare_snapshot(&subordinate, "subordinate_decision_subordinate_db");

    write_file(&source_root, "payload.bin", "source-content");
    write_file(&subordinate_root, "payload.bin", "subordinate-content");

    let path = kitchensync::RelPath::new("payload.bin").unwrap();
    source
        .transport
        .set_mod_time(
            &path,
            kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        )
        .unwrap();
    subordinate
        .transport
        .set_mod_time(
            &path,
            kitchensync::Timestamp("2024-01-01_00-00-30_000000Z".to_string()),
        )
        .unwrap();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &subordinate,
            snapshot: &mut subordinate_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);
    assert_eq!(report.copies.succeeded, 2);
    assert_eq!(
        fs::read_to_string(destination_root.join("payload.bin")).unwrap(),
        "source-content"
    );
    assert_eq!(
        fs::read_to_string(subordinate_root.join("payload.bin")).unwrap(),
        "source-content"
    );
    assert!(source_snapshot.store.lookup(&path).unwrap().is_some());
    assert!(destination_snapshot.store.lookup(&path).unwrap().is_some());
    assert!(subordinate_snapshot.store.lookup(&path).unwrap().is_some());
}

#[test]
fn sync_run_copies_winner_of_file_competition_to_smaller_file_peer() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("winner_source");
    let destination_root = next_test_root("winner_destination");

    let source = make_peer_session(
        70_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "winner_source_db");
    let destination = make_peer_session(
        70_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "winner_destination_db");

    write_file(&source_root, "payload.bin", "short");
    write_file(&destination_root, "payload.bin", "longer payload text");

    let path = kitchensync::RelPath::new("payload.bin").unwrap();
    destination
        .transport
        .set_mod_time(
            &path,
            kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        )
        .unwrap();
    source
        .transport
        .set_mod_time(
            &path,
            kitchensync::Timestamp("2024-01-01_00-00-10_000000Z".to_string()),
        )
        .unwrap();
    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.copies.succeeded, 1);
    assert_eq!(
        fs::read_to_string(destination_root.join("payload.bin")).unwrap(),
        "short"
    );
    assert!(destination_snapshot.store.lookup(&path).unwrap().is_some());
}

#[test]
fn sync_run_successful_copy_sets_destination_last_seen() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("success_copy_last_seen_source");
    let destination_root = next_test_root("success_copy_last_seen_destination");

    let source = make_peer_session(
        88_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "success_copy_last_seen_source_db");
    let destination = make_peer_session(
        88_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "success_copy_last_seen_destination_db");

    write_file(&source_root, "payload.bin", "sync success");
    let path = kitchensync::RelPath::new("payload.bin").unwrap();

    assert!(destination_snapshot.store.lookup(&path).unwrap().is_none());

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.copies.succeeded, 1);

    let destination_row = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(
        destination_row.kind,
        kitchensync::snapshot::SnapshotEntryKind::File
    );
    assert!(destination_row.last_seen.is_some());
    assert!(destination_row.deleted_time.is_none());
}

#[test]
fn sync_run_skips_subdirectory_when_listing_fails_for_all_contributing_peers() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingRecoverOperationExecutor {
        delegate: &base_executor,
        fail_recover_directories: vec!["shared".to_string()],
    };
    let scheduler = make_scheduler();
    let progress = NullSink;

    let shared_source_root = next_test_root("failshared_source");
    let shared_destination_root = next_test_root("failshared_destination");

    let shared_source = make_peer_session(
        80_001,
        &shared_source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&shared_source, "failshared_source_db");
    let shared_destination = make_peer_session(
        80_002,
        &shared_destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&shared_destination, "failshared_destination_db");

    fs::create_dir_all(shared_source_root.join("shared")).unwrap();
    fs::create_dir_all(shared_destination_root.join("shared")).unwrap();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &shared_source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &shared_destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &progress,
        progress: &progress,
    });

    assert!(!report.completed);
    assert_eq!(report.traversal.scanned_directories, 2);
    assert!(report
        .skipped
        .iter()
        .any(|entry| entry.directory.as_str() == "shared"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed
            )));
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        kitchensync::sync::SyncFailure::SwapRecovery {
            canon: false,
            directory,
            ..
        } if directory.as_str() == "shared"
    )));
}

#[test]
fn sync_run_dry_run_copies_without_peer_mutation() {
    let config = make_run_config(true, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("dry_run_source");
    let destination_root = next_test_root("dry_run_destination");

    let source = make_peer_session(
        90_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "dry_run_source_db");
    let destination = make_peer_session(
        90_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "dry_run_destination_db");

    write_file(&source_root, "ghost.txt", "dry");

    let path = kitchensync::RelPath::new("ghost.txt").unwrap();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.copies.succeeded, 1);
    assert!(!destination_root.join("ghost.txt").exists());

    assert!(source_snapshot.store.lookup(&path).unwrap().is_some());
    assert!(destination_snapshot.store.lookup(&path).unwrap().is_some());
}

#[test]
fn sync_run_skips_canon_listing_failure_subtree_for_all_peers() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingRecoverOperationExecutorByPeer {
        delegate: &base_executor,
        fail_recover_for_peer_and_directory: vec![(70_001, "shared".to_string())],
    };
    let scheduler = make_scheduler();

    let canon_root = next_test_root("canon_fail_source_root");
    let contributor_root = next_test_root("canon_fail_contributor_root");

    let canon = make_peer_session(70_001, &canon_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "canon_fail_source_db");
    let contributor = make_peer_session(
        70_002,
        &contributor_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut contributor_snapshot = prepare_snapshot(&contributor, "canon_fail_contributor_db");

    fs::create_dir_all(canon_root.join("shared")).unwrap();
    fs::create_dir_all(contributor_root.join("shared")).unwrap();
    write_file(
        &contributor_root,
        "shared/displaced.txt",
        "to be kept while skipped",
    );
    write_file(&contributor_root, "common.txt", "common-root-entry");
    write_file(&canon_root, "common.txt", "common-root-entry");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &canon,
            snapshot: &mut canon_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &contributor,
            snapshot: &mut contributor_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    let shared_path = kitchensync::RelPath::new("shared/displaced.txt").unwrap();
    let skipped = report
        .skipped
        .iter()
        .find(|entry| entry.directory.as_str() == "shared")
        .expect("shared subtree should be skipped");

    assert!(!report.completed);
    assert_eq!(report.traversal.scanned_directories, 2);
    assert_eq!(report.copies.succeeded, 0);
    assert!(matches!(
        skipped.reason,
        kitchensync::sync::SkippedSubtreeReason::CanonListingUnavailable { peer_id: 70_001 }
    ));
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        kitchensync::sync::SyncFailure::SwapRecovery {
            peer_id: 70_001,
            directory,
            canon: true,
            ..
        } if directory.as_str() == "shared"
    )));

    assert!(contributor_root.join("shared/displaced.txt").exists());
    assert!(!canon_root.join("shared/displaced.txt").exists());
    assert!(contributor_snapshot
        .store
        .lookup(&shared_path)
        .unwrap()
        .is_none());
    assert!(canon_snapshot.store.lookup(&shared_path).unwrap().is_none());
}

#[derive(Clone)]
struct FailingCopyExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_copy_paths: Vec<String>,
}

impl<'a> kitchensync::operations::OperationExecutor for FailingCopyExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        self.delegate.recover_directory_swaps(peer, directory)
    }

    fn displace_to_bak(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
        timestamp: kitchensync::Timestamp,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DisplacementReport> {
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.delegate.create_directory(peer, path)
    }

    fn cleanup_retention(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
        now: kitchensync::Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::CleanupReport> {
        self.delegate
            .cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)
    }

    fn execute_copy_attempt(
        &self,
        source_peer: &kitchensync::PeerSession,
        source_path: &kitchensync::RelPath,
        destination_peer: &kitchensync::PeerSession,
        destination_path: &kitchensync::RelPath,
        winning_meta: &kitchensync::EntryMeta,
    ) -> kitchensync::CopyResult {
        if self
            .fail_copy_paths
            .iter()
            .any(|path| path == destination_path.as_str())
        {
            return kitchensync::CopyResult {
                source_peer_id: source_peer.id,
                source_path: source_path.clone(),
                destination_peer_id: destination_peer.id,
                destination_path: destination_path.clone(),
                bytes_copied: 0,
                completed: false,
                failed_phase: Some(kitchensync::TransferPhase::ReadSource),
                error: Some(kitchensync::TransportError::IoError),
            };
        }

        self.delegate.execute_copy_attempt(
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

#[test]
fn sync_run_reports_failed_copy_as_sync_failure() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingCopyExecutor {
        delegate: &base_executor,
        fail_copy_paths: vec!["payload.bin".to_string()],
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("sync_copy_fail_source");
    let destination_root = next_test_root("sync_copy_fail_destination");

    let source = make_peer_session(
        91_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "sync_copy_fail_source_db");
    let destination = make_peer_session(
        91_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "sync_copy_fail_destination_db");

    write_file(&source_root, "payload.bin", "sync fail");
    let path = kitchensync::RelPath::new("payload.bin").unwrap();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination,
            snapshot: &mut destination_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert_eq!(report.copies.failed, 1);
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(failure, kitchensync::sync::SyncFailure::Copy { result } if result.destination_path == path)));
    assert!(!destination_root.join("payload.bin").exists());
    let row = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.kind, kitchensync::snapshot::SnapshotEntryKind::File);
    assert_eq!(row.last_seen, None);
}
