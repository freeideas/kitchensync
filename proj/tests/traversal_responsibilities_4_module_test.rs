use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

#[derive(Default, Clone)]
struct NullSink;

impl kitchensync::runtime::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::runtime::DiagnosticEvent) {}
}

impl kitchensync::runtime::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::runtime::ProgressEvent) {}
}

impl kitchensync::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::ProgressEvent) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceEventKind {
    ListDir,
    CreateDirectory,
    Displace,
    Cleanup,
}

#[derive(Clone, Debug)]
struct TraceEvent {
    kind: TraceEventKind,
    peer_id: kitchensync::PeerId,
    path: String,
}

#[derive(Clone, Default)]
struct TestTrace {
    events: Arc<Mutex<Vec<TraceEvent>>>,
}

impl TestTrace {
    fn push_event(&self, kind: TraceEventKind, peer_id: kitchensync::PeerId, path: &str) {
        let mut events = self.events.lock().expect("trace events lock poisoned");
        events.push(TraceEvent {
            kind,
            peer_id,
            path: path.to_string(),
        });
    }

    fn events(&self) -> Vec<TraceEvent> {
        self.events
            .lock()
            .expect("trace events lock poisoned")
            .clone()
    }
}

#[derive(Clone)]
struct TracingTransport {
    peer_id: kitchensync::PeerId,
    inner: kitchensync::TransportHandle,
    trace: TestTrace,
    fail_list_dirs: Vec<String>,
}

impl kitchensync::TransportBackend for TracingTransport {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        let path_text = path.as_str();
        self.trace
            .push_event(TraceEventKind::ListDir, self.peer_id, path_text);

        if self.fail_list_dirs.iter().any(|target| target == path_text) {
            return Err(kitchensync::TransportError::PermissionDenied);
        }

        self.inner.list_dir(path)
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
struct CaseVariantListingTransport {
    inner: kitchensync::TransportHandle,
}

impl kitchensync::TransportBackend for CaseVariantListingTransport {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        match path.as_str() {
            "" => Ok(["a", "A", "b"]
                .into_iter()
                .map(|name| kitchensync::EntryMeta {
                    name: name.to_string(),
                    kind: kitchensync::EntryKind::Directory,
                    mod_time: kitchensync::Timestamp("2024-01-01T00:00:00Z".to_string()),
                    byte_size: -1,
                })
                .collect()),
            "a" | "A" | "b" => Ok(Vec::new()),
            _ => self.inner.list_dir(path),
        }
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
    fail_displace: Vec<(kitchensync::PeerId, String)>,
}

impl<'a> TracingOperationExecutor<'a> {
    fn should_fail_displace(&self, peer_id: kitchensync::PeerId, path: &str) -> bool {
        self.fail_displace
            .iter()
            .any(|(target_peer, target_path)| *target_peer == peer_id && target_path == path)
    }
}

impl<'a> kitchensync::operations::OperationExecutor for TracingOperationExecutor<'a> {
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
        self.trace
            .push_event(TraceEventKind::Displace, peer.id, path.as_str());

        if self.should_fail_displace(peer.id, path.as_str()) {
            return Err(kitchensync::operations::OperationError {
                peer_id: peer.id,
                context: kitchensync::operations::OperationErrorContext::DisplaceToBak {
                    path: path.clone(),
                },
                error: kitchensync::TransportError::IoError,
            });
        }

        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.trace
            .push_event(TraceEventKind::CreateDirectory, peer.id, path.as_str());
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
        self.trace
            .push_event(TraceEventKind::Cleanup, peer.id, directory.as_str());
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
    path.push(format!(
        "kitchensync-traversal-responsibilities-4-{normalized}-{seq}"
    ));

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

fn read_file(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
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

fn make_peer_session_with_transport(
    id: kitchensync::PeerId,
    root: &Path,
    role: kitchensync::EffectivePeerRole,
    backend: impl kitchensync::TransportBackend + 'static,
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

#[test]
fn sync_run_orders_directory_candidates_case_insensitively_with_case_tie_breaker() {
    let config = make_run_config(true, 1);
    let trace = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_displace: Vec::new(),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("responsibility_4_case_order_source");
    let destination_root = next_test_root("responsibility_4_case_order_destination");

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

    let source = make_peer_session_with_transport(
        401_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        CaseVariantListingTransport {
            inner: source_transport,
        },
    );
    let mut source_snapshot = prepare_snapshot(&source, "responsibility_4_case_order_source_db");
    let destination = make_peer_session(
        401_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "responsibility_4_case_order_destination_db");

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
    assert_eq!(report.traversal.decided_entries, 3);

    let create_calls: Vec<String> = trace
        .events()
        .into_iter()
        .filter(|event| event.kind == TraceEventKind::CreateDirectory && event.peer_id == 401_002)
        .map(|event| event.path)
        .collect();

    assert_eq!(create_calls, vec!["A", "a", "b"]);
}

#[test]
fn sync_run_recurses_into_child_directory_only_for_successful_directory_peers() {
    let config = make_run_config(false, 1);
    let trace = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_displace: vec![(402_002, "child".to_string())],
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("responsibility_4_child_peer_set_source");
    let destination_root = next_test_root("responsibility_4_child_peer_set_destination");

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
        402_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        TracingTransport {
            peer_id: 402_001,
            inner: source_transport,
            trace: trace.clone(),
            fail_list_dirs: Vec::new(),
        },
    );
    let mut source_snapshot =
        prepare_snapshot(&source, "responsibility_4_child_peer_set_source_db");

    let destination = make_peer_session_with_transport(
        402_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Subordinate,
        TracingTransport {
            peer_id: 402_002,
            inner: destination_transport,
            trace: trace.clone(),
            fail_list_dirs: Vec::new(),
        },
    );
    let mut destination_snapshot = prepare_snapshot(
        &destination,
        "responsibility_4_child_peer_set_destination_db",
    );

    fs::create_dir_all(source_root.join("child")).unwrap();
    write_file(&source_root, "child/inner.txt", "winner");
    write_file(&destination_root, "child", "occupied-by-file");

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

    let events = trace.events();
    assert!(events.iter().any(|event| {
        event.kind == TraceEventKind::Displace && event.peer_id == 402_002 && event.path == "child"
    }));
    assert!(!events.iter().any(|event| {
        event.kind == TraceEventKind::ListDir && event.peer_id == 402_002 && event.path == "child"
    }));
    assert!(events.iter().any(|event| {
        event.kind == TraceEventKind::ListDir && event.peer_id == 402_001 && event.path == "child"
    }));

    assert!(destination_root.join("child").is_file());
    assert!(!destination_root.join("child/inner.txt").exists());
}

#[test]
fn sync_run_runs_bak_tmp_cleanup_after_directory_processing_in_normal_mode() {
    let config = make_run_config(false, 1);
    let trace = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_displace: Vec::new(),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("responsibility_4_cleanup_source");
    let destination_root = next_test_root("responsibility_4_cleanup_destination");

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
        403_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        TracingTransport {
            peer_id: 403_001,
            inner: source_transport,
            trace: trace.clone(),
            fail_list_dirs: Vec::new(),
        },
    );
    let mut source_snapshot = prepare_snapshot(&source, "responsibility_4_cleanup_source_db");

    let destination = make_peer_session_with_transport(
        403_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Subordinate,
        TracingTransport {
            peer_id: 403_002,
            inner: destination_transport,
            trace: trace.clone(),
            fail_list_dirs: Vec::new(),
        },
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "responsibility_4_cleanup_destination_db");

    fs::create_dir_all(source_root.join("child")).unwrap();
    fs::create_dir_all(destination_root.join("child")).unwrap();
    write_file(&source_root, ".kitchensync/BAK/old.txt", "source-metadata");
    write_file(
        &destination_root,
        ".kitchensync/BAK/old.txt",
        "destination-metadata",
    );

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

    let events = trace.events();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == TraceEventKind::Cleanup && event.path == "")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == TraceEventKind::Cleanup && event.path == "child")
            .count(),
        2
    );
    assert!(!events
        .iter()
        .any(|event| event.kind == TraceEventKind::ListDir && event.path == ".kitchensync"));

    for peer_id in [403_001_u64, 403_002_u64] {
        let cleanup_root = first_matching_index(&events, |event| {
            event.kind == TraceEventKind::Cleanup && event.peer_id == peer_id && event.path == ""
        });
        let child_list = first_matching_index(&events, |event| {
            event.kind == TraceEventKind::ListDir
                && event.peer_id == peer_id
                && event.path == "child"
        });
        assert!(cleanup_root > child_list);
    }

    assert_eq!(
        read_file(&destination_root, ".kitchensync/BAK/old.txt"),
        "destination-metadata"
    );
}

#[test]
fn sync_run_skips_bak_tmp_cleanup_in_dry_run_mode() {
    let config = make_run_config(true, 1);
    let trace = TestTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_displace: Vec::new(),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("responsibility_4_cleanup_dry_run_source");
    let destination_root = next_test_root("responsibility_4_cleanup_dry_run_destination");

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
        404_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        TracingTransport {
            peer_id: 404_001,
            inner: source_transport,
            trace: trace.clone(),
            fail_list_dirs: Vec::new(),
        },
    );
    let mut source_snapshot =
        prepare_snapshot(&source, "responsibility_4_cleanup_dry_run_source_db");

    let destination = make_peer_session_with_transport(
        404_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Subordinate,
        TracingTransport {
            peer_id: 404_002,
            inner: destination_transport,
            trace: trace.clone(),
            fail_list_dirs: Vec::new(),
        },
    );
    let mut destination_snapshot = prepare_snapshot(
        &destination,
        "responsibility_4_cleanup_dry_run_destination_db",
    );

    fs::create_dir_all(source_root.join("child")).unwrap();
    fs::create_dir_all(destination_root.join("child")).unwrap();
    write_file(&source_root, ".kitchensync/TMP/old.txt", "source-tmp");
    write_file(
        &destination_root,
        ".kitchensync/TMP/old.txt",
        "destination-tmp",
    );

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

    assert!(!trace
        .events()
        .into_iter()
        .any(|event| event.kind == TraceEventKind::Cleanup));
}
