use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Barrier, Mutex,
};

#[derive(Default)]
struct NullSink;

impl kitchensync::runtime::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::runtime::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::ProgressEvent) {}
}

impl kitchensync::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::ProgressEvent) {}
}

#[derive(Default, Clone)]
struct RecordingProgressSink {
    events: Arc<Mutex<Vec<kitchensync::ProgressEvent>>>,
}

impl kitchensync::runtime::ProgressSink for RecordingProgressSink {
    fn publish(&self, event: kitchensync::ProgressEvent) {
        self.events
            .lock()
            .expect("progress lock poisoned")
            .push(event);
    }
}

impl kitchensync::ProgressSink for RecordingProgressSink {
    fn publish(&self, event: kitchensync::ProgressEvent) {
        self.events
            .lock()
            .expect("progress lock poisoned")
            .push(event);
    }
}

impl RecordingProgressSink {
    fn snapshot(&self) -> Vec<kitchensync::ProgressEvent> {
        self.events.lock().expect("progress lock poisoned").clone()
    }
}

#[derive(Debug, Clone)]
struct ListConcurrencyState {
    active: usize,
    max_active: usize,
}

impl Default for ListConcurrencyState {
    fn default() -> Self {
        Self {
            active: 0,
            max_active: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TraceEventKind {
    ListStart,
    ListEnd,
    SwapRecovery,
    Displace,
}

#[derive(Clone, Debug)]
struct TraceEvent {
    seq: usize,
    kind: TraceEventKind,
    peer_id: kitchensync::PeerId,
    path: String,
}

#[derive(Clone)]
struct TestTrace {
    events: Arc<Mutex<Vec<TraceEvent>>>,
    list_state: Arc<Mutex<HashMap<String, ListConcurrencyState>>>,
}

impl Default for TestTrace {
    fn default() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            list_state: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl TestTrace {
    fn push_event(&self, kind: TraceEventKind, peer_id: kitchensync::PeerId, path: &str) {
        let mut events = self.events.lock().expect("trace events lock poisoned");
        let seq = events.len();
        events.push(TraceEvent {
            seq,
            kind,
            peer_id,
            path: path.to_string(),
        });
    }

    fn list_start(&self, peer_id: kitchensync::PeerId, path: &kitchensync::RelPath) {
        let path = path.as_str().to_string();
        {
            let mut state = self.list_state.lock().expect("list-state lock poisoned");
            let entry = state
                .entry(path.clone())
                .or_insert_with(ListConcurrencyState::default);
            entry.active += 1;
            if entry.active > entry.max_active {
                entry.max_active = entry.active;
            }
        }
        self.push_event(TraceEventKind::ListStart, peer_id, &path);
    }

    fn list_end(&self, peer_id: kitchensync::PeerId, path: &kitchensync::RelPath) {
        let path = path.as_str().to_string();
        {
            let mut state = self.list_state.lock().expect("list-state lock poisoned");
            if let Some(entry) = state.get_mut(&path) {
                entry.active = entry.active.saturating_sub(1);
            }
        }
        self.push_event(TraceEventKind::ListEnd, peer_id, &path);
    }

    fn record_recover(&self, peer_id: kitchensync::PeerId, directory: &kitchensync::RelPath) {
        self.push_event(TraceEventKind::SwapRecovery, peer_id, directory.as_str());
    }

    fn record_displace(&self, peer_id: kitchensync::PeerId, path: &kitchensync::RelPath) {
        self.push_event(TraceEventKind::Displace, peer_id, path.as_str());
    }

    fn max_list_concurrency(&self, path: &str) -> usize {
        self.list_state
            .lock()
            .expect("list-state lock poisoned")
            .get(path)
            .map_or(0, |state| state.max_active)
    }

    fn events(&self) -> Vec<TraceEvent> {
        let mut events = self
            .events
            .lock()
            .expect("trace events lock poisoned")
            .clone();
        events.sort_by_key(|event| event.seq);
        events
    }
}

#[derive(Clone)]
struct TracingTransport {
    peer_id: kitchensync::PeerId,
    inner: kitchensync::TransportHandle,
    trace: TestTrace,
}

impl kitchensync::TransportBackend for TracingTransport {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        self.trace.list_start(self.peer_id, path);
        let entries = self.inner.list_dir(path);
        self.trace.list_end(self.peer_id, path);
        entries
    }

    fn stat(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::EntryMeta, kitchensync::TransportError> {
        self.inner.stat(path)
    }

    fn open_read(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportRead, kitchensync::TransportError> {
        self.inner.open_read(path)
    }

    fn open_write(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportWrite, kitchensync::TransportError> {
        self.inner.open_write(path)
    }

    fn rename_no_overwrite(
        &self,
        src: &kitchensync::RelPath,
        dst: &kitchensync::RelPath,
    ) -> Result<(), kitchensync::TransportError> {
        self.inner.rename_no_overwrite(src, dst)
    }

    fn delete_file(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.inner.delete_file(path)
    }

    fn create_dir(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.inner.create_dir(path)
    }

    fn delete_dir(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.inner.delete_dir(path)
    }

    fn set_mod_time(
        &self,
        path: &kitchensync::RelPath,
        time: kitchensync::Timestamp,
    ) -> Result<(), kitchensync::TransportError> {
        self.inner.set_mod_time(path, time)
    }
}

#[derive(Clone)]
struct BlockingTracingTransport {
    peer_id: kitchensync::PeerId,
    inner: kitchensync::TransportHandle,
    trace: TestTrace,
    list_gate: Arc<Barrier>,
}

impl kitchensync::TransportBackend for BlockingTracingTransport {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        self.trace.list_start(self.peer_id, path);
        self.list_gate.wait();
        let entries = self.inner.list_dir(path);
        self.trace.list_end(self.peer_id, path);
        entries
    }

    fn stat(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::EntryMeta, kitchensync::TransportError> {
        self.inner.stat(path)
    }

    fn open_read(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportRead, kitchensync::TransportError> {
        self.inner.open_read(path)
    }

    fn open_write(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportWrite, kitchensync::TransportError> {
        self.inner.open_write(path)
    }

    fn rename_no_overwrite(
        &self,
        src: &kitchensync::RelPath,
        dst: &kitchensync::RelPath,
    ) -> Result<(), kitchensync::TransportError> {
        self.inner.rename_no_overwrite(src, dst)
    }

    fn delete_file(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.inner.delete_file(path)
    }

    fn create_dir(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.inner.create_dir(path)
    }

    fn delete_dir(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.inner.delete_dir(path)
    }

    fn set_mod_time(
        &self,
        path: &kitchensync::RelPath,
        time: kitchensync::Timestamp,
    ) -> Result<(), kitchensync::TransportError> {
        self.inner.set_mod_time(path, time)
    }
}

struct TracingOperationExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    trace: TestTrace,
}

impl<'a> kitchensync::operations::OperationExecutor for TracingOperationExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        self.trace.record_recover(peer.id, directory);
        self.delegate.recover_directory_swaps(peer, directory)
    }

    fn displace_to_bak(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
        timestamp: kitchensync::Timestamp,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DisplacementReport> {
        self.trace.record_displace(peer.id, path);
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
    let normalized = test_name.replace(['\\', '/'], "_");
    let mut path = std::env::temp_dir();
    path.push(format!("kitchensync-traversal-1-{normalized}-{seq}"));

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

fn first_matching_index(events: &[TraceEvent], matcher: impl Fn(&TraceEvent) -> bool) -> usize {
    events
        .iter()
        .position(matcher)
        .unwrap_or_else(|| panic!("expected matching trace event"))
}

fn normalize_progress_directory(path: &kitchensync::RelPath) -> String {
    if path.as_str().is_empty() {
        ".".to_string()
    } else {
        path.as_str().to_string()
    }
}

#[test]
fn sync_run_reports_scanned_directory_paths_in_preorder() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let timeline = TestTrace::default();
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: timeline,
    };
    let progress = RecordingProgressSink::default();
    let scheduler = make_scheduler();

    let source_root = next_test_root("traversal_preorder_source");
    let destination_root = next_test_root("traversal_preorder_destination");

    let source = make_peer_session(
        11_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "traversal_preorder_source_snapshot");
    let destination = make_peer_session(
        11_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "traversal_preorder_destination_snapshot");

    write_file(&source_root, "alpha/child/file.txt", "nested");
    write_file(&source_root, "omega/file.txt", "nested");

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

    let scanned = progress
        .snapshot()
        .into_iter()
        .filter_map(|event| match event {
            kitchensync::ProgressEvent::Scanning { directory } => Some(directory),
            _ => None,
        })
        .map(|directory| normalize_progress_directory(&directory))
        .collect::<Vec<_>>();

    assert_eq!(scanned, vec![".", "alpha", "alpha/child", "omega"]);
    assert!(report.completed);
    assert_eq!(report.traversal.scanned_directories, 4);

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_requests_swap_recovery_for_all_active_peers_before_listing_at_each_directory_level() {
    let config = make_run_config(false, 1);
    let timeline = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: timeline.clone(),
    };
    let progress = NullSink;
    let scheduler = make_scheduler();

    let source_root = next_test_root("traversal_recovery_root_source");
    let destination_root = next_test_root("traversal_recovery_root_destination");

    let source_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&source_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();
    let destination_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&destination_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();

    let source = make_peer_session_with_transport(
        12_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        TracingTransport {
            peer_id: 12_001,
            inner: source_transport,
            trace: timeline.clone(),
        },
    );
    let mut source_snapshot = prepare_snapshot(&source, "traversal_recovery_root_source_snapshot");

    let destination = make_peer_session_with_transport(
        12_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
        TracingTransport {
            peer_id: 12_002,
            inner: destination_transport,
            trace: timeline.clone(),
        },
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "traversal_recovery_root_destination_snapshot");

    write_file(&source_root, "adir/file.txt", "root");

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

    let events = timeline.events();
    for peer_id in [12_001_u64, 12_002_u64] {
        for directory in ["", "adir"] {
            let recover_index = first_matching_index(&events, |event| {
                event.kind == TraceEventKind::SwapRecovery
                    && event.peer_id == peer_id
                    && event.path == directory
            });
            let list_index = first_matching_index(&events, |event| {
                event.kind == TraceEventKind::ListStart
                    && event.peer_id == peer_id
                    && event.path == directory
            });
            assert!(recover_index < list_index);
        }
    }

    let recover_calls = timeline
        .events()
        .into_iter()
        .filter(|event| event.kind == TraceEventKind::SwapRecovery)
        .count();
    assert_eq!(recover_calls, 4);

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_skips_swap_recovery_in_dry_run() {
    let config = make_run_config(true, 1);
    let timeline = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: timeline.clone(),
    };
    let progress = NullSink;
    let scheduler = make_scheduler();

    let source_root = next_test_root("traversal_dryrun_recovery_source");
    let destination_root = next_test_root("traversal_dryrun_recovery_destination");

    let source = make_peer_session(
        13_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot =
        prepare_snapshot(&source, "traversal_dryrun_recovery_source_snapshot");
    let destination = make_peer_session(
        13_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(
        &destination,
        "traversal_dryrun_recovery_destination_snapshot",
    );

    write_file(&source_root, "adir/file.txt", "root");

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
    assert_eq!(
        timeline
            .events()
            .into_iter()
            .filter(|event| event.kind == TraceEventKind::SwapRecovery)
            .count(),
        0
    );

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_starts_active_peer_listings_before_waiting_for_results() {
    let config = make_run_config(true, 1);
    let timeline = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: timeline.clone(),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("traversal_listing_concurrency_source");
    let destination_root = next_test_root("traversal_listing_concurrency_destination");

    let source_base_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&source_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();
    let destination_base_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&destination_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();
    let list_gate = Arc::new(Barrier::new(2));

    let source = make_peer_session_with_transport(
        14_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        BlockingTracingTransport {
            peer_id: 14_001,
            inner: source_base_transport,
            trace: timeline.clone(),
            list_gate: Arc::clone(&list_gate),
        },
    );
    let mut source_snapshot =
        prepare_snapshot(&source, "traversal_listing_concurrency_source_snapshot");

    let destination = make_peer_session_with_transport(
        14_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
        BlockingTracingTransport {
            peer_id: 14_002,
            inner: destination_base_transport,
            trace: timeline.clone(),
            list_gate,
        },
    );
    let mut destination_snapshot = prepare_snapshot(
        &destination,
        "traversal_listing_concurrency_destination_snapshot",
    );

    write_file(&source_root, "adir/file.txt", "nested");

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
    assert!(timeline.max_list_concurrency("") >= 2);
    assert!(timeline.max_list_concurrency("adir") >= 2);

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_processes_non_directory_entries_before_recurse_into_directory() {
    let config = make_run_config(false, 1);
    let timeline = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: timeline.clone(),
    };
    let copy_progress = RecordingProgressSink::default();
    let scheduler = kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: 2,
            retries_copy: 2,
        },
        NullSink,
        copy_progress.clone(),
    );

    let source_root = next_test_root("traversal_inline_source");
    let destination_root = next_test_root("traversal_inline_destination");

    let source_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&source_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();
    let destination_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&destination_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();

    let source = make_peer_session_with_transport(
        15_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        TracingTransport {
            peer_id: 15_001,
            inner: source_transport,
            trace: timeline.clone(),
        },
    );
    let mut source_snapshot = prepare_snapshot(&source, "traversal_inline_source_snapshot");

    let destination = make_peer_session_with_transport(
        15_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
        TracingTransport {
            peer_id: 15_002,
            inner: destination_transport,
            trace: timeline.clone(),
        },
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "traversal_inline_destination_snapshot");

    write_file(&source_root, "adir/file.txt", "in child");
    write_file(&source_root, "root-file.txt", "winner-content");
    write_file(&destination_root, "root-file.txt", "old");

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

    let events = timeline.events();
    first_matching_index(&events, |event| {
        event.kind == TraceEventKind::ListStart && event.path == "adir"
    });

    assert!(copy_progress.snapshot().into_iter().any(|event| matches!(
        event,
        kitchensync::ProgressEvent::CopyStarted { basename, .. } if basename == "root-file.txt"
    )));
    assert_eq!(
        fs::read_to_string(destination_root.join("root-file.txt")).unwrap(),
        "winner-content"
    );

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}
