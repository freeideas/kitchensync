use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
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

#[derive(Clone, Default)]
struct OperationTrace {
    copy_calls: Arc<AtomicUsize>,
    displace_calls: Arc<AtomicUsize>,
    create_calls: Arc<AtomicUsize>,
    cleanup_calls: Arc<AtomicUsize>,
    copy_paths: Arc<Mutex<Vec<String>>>,
    cleanup_paths: Arc<Mutex<Vec<String>>>,
}

impl OperationTrace {
    fn record_copy(
        &self,
        source_path: &kitchensync::RelPath,
        destination_path: &kitchensync::RelPath,
    ) {
        self.copy_calls.fetch_add(1, Ordering::SeqCst);
        let mut copy_paths = self.copy_paths.lock().expect("copy path lock poisoned");
        copy_paths.push(format!(
            "{}=>{}",
            source_path.as_str(),
            destination_path.as_str()
        ));
    }

    fn record_displace(&self) {
        self.displace_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn record_create(&self) {
        self.create_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn record_cleanup(&self, directory: &kitchensync::RelPath) {
        self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
        let mut cleanup_paths = self
            .cleanup_paths
            .lock()
            .expect("cleanup path lock poisoned");
        cleanup_paths.push(directory.as_str().to_string());
    }

    fn copy_calls(&self) -> usize {
        self.copy_calls.load(Ordering::SeqCst)
    }

    fn displace_calls(&self) -> usize {
        self.displace_calls.load(Ordering::SeqCst)
    }

    fn create_calls(&self) -> usize {
        self.create_calls.load(Ordering::SeqCst)
    }

    fn cleanup_calls(&self) -> usize {
        self.cleanup_calls.load(Ordering::SeqCst)
    }

    fn copy_paths(&self) -> Vec<String> {
        self.copy_paths
            .lock()
            .expect("copy path lock poisoned")
            .clone()
    }

    fn cleanup_paths(&self) -> Vec<String> {
        self.cleanup_paths
            .lock()
            .expect("cleanup path lock poisoned")
            .clone()
    }
}

struct TracingOperationExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    trace: OperationTrace,
    fail_recover_directories: Vec<String>,
    fail_recover_peer_ids: Option<Vec<kitchensync::PeerId>>,
}

impl<'a> TracingOperationExecutor<'a> {
    fn should_fail_recovery(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> bool {
        if !self
            .fail_recover_directories
            .iter()
            .any(|target| target == directory.as_str())
        {
            return false;
        }

        match &self.fail_recover_peer_ids {
            Some(ids) => ids.contains(&peer.id),
            None => true,
        }
    }
}

impl<'a> kitchensync::operations::OperationExecutor for TracingOperationExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        if self.should_fail_recovery(peer, directory) {
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
        self.trace.record_displace();
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.trace.record_create();
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
        self.trace.record_cleanup(directory);
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
        self.trace.record_copy(source_path, destination_path);
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
        "kitchensync-traversal-boundaries-2-{normalized}-{seq}"
    ));

    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }

    fs::create_dir_all(&path).unwrap();
    path
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

fn create_directory(root: &Path, rel: &str) {
    fs::create_dir_all(root.join(rel)).unwrap();
}

fn write_file(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn has_path_or_descendant(paths: &[String], prefix: &str) -> bool {
    let nested = format!("{}/", prefix);
    paths
        .iter()
        .any(|path| path == prefix || path.starts_with(&nested))
}

#[test]
fn traversal_boundaries_2_directory_mutation_actions_do_not_become_copy_attempts() {
    let config = make_run_config(false, 1);
    let trace = OperationTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_recover_directories: Vec::new(),
        fail_recover_peer_ids: None,
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("dir_mutation_not_copy_queued_source");
    let destination_root = next_test_root("dir_mutation_not_copy_queued_destination");

    let source = make_peer_session(
        1_000_001,
        &source_root,
        kitchensync::EffectivePeerRole::Canon,
    );
    let mut source_snapshot = prepare_snapshot(&source, "dir_mutation_not_copy_queued_source_db");

    let destination = make_peer_session(
        1_000_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "dir_mutation_not_copy_queued_destination_db");

    create_directory(&source_root, "dir_conflict");
    write_file(&destination_root, "dir_conflict", "blocked by file");
    write_file(&source_root, "payload.txt", "winner");
    write_file(&destination_root, "payload.txt", "old");

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
    assert_eq!(report.traversal.decided_entries, 2);
    assert_eq!(report.traversal.enqueued_copies, 1);
    assert_eq!(report.copies.succeeded, 1);
    assert_eq!(trace.copy_calls(), 1);
    assert_eq!(trace.displace_calls(), 1);
    assert_eq!(trace.create_calls(), 1);

    let copies = trace.copy_paths();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0], "payload.txt=>payload.txt");
    assert!(!has_path_or_descendant(&copies, "dir_conflict"));

    assert!(destination_root.join("payload.txt").exists());
    assert_eq!(
        fs::read_to_string(destination_root.join("payload.txt")).unwrap(),
        "winner"
    );
    assert!(destination_root.join("dir_conflict").is_dir());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn traversal_boundaries_2_excluded_paths_do_not_cause_child_traversal_or_row_updates() {
    let config = kitchensync::RunConfig {
        excludes: vec![kitchensync::RelPath::new("skipdir").unwrap()],
        ..make_run_config(false, 1)
    };

    let trace = OperationTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_recover_directories: Vec::new(),
        fail_recover_peer_ids: None,
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("excluded_no_descend_source");
    let destination_root = next_test_root("excluded_no_descend_destination");

    let source = make_peer_session(
        1_100_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "excluded_no_descend_source_db");

    let destination = make_peer_session(
        1_100_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "excluded_no_descend_destination_db");

    write_file(&source_root, "skipdir/inner.txt", "source never sync");
    write_file(&source_root, "keep.txt", "keep me");
    write_file(&destination_root, "skipdir/inner.txt", "keep destination");

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
    assert_eq!(report.traversal.enqueued_copies, 1);
    assert_eq!(trace.copy_calls(), 1);
    assert_eq!(trace.displace_calls(), 0);
    assert_eq!(trace.create_calls(), 0);
    assert_eq!(trace.cleanup_calls(), 2);

    let cleanup_paths = trace.cleanup_paths();
    assert!(!has_path_or_descendant(&cleanup_paths, "skipdir"));

    let copies = trace.copy_paths();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0], "keep.txt=>keep.txt");
    assert_eq!(
        fs::read_to_string(destination_root.join("keep.txt")).unwrap(),
        "keep me"
    );
    assert_eq!(
        fs::read_to_string(destination_root.join("skipdir/inner.txt")).unwrap(),
        "keep destination"
    );

    let excluded = kitchensync::RelPath::new("skipdir/inner.txt").unwrap();
    assert!(source_snapshot.store.lookup(&excluded).unwrap().is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded)
        .unwrap()
        .is_none());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn traversal_boundaries_2_no_contributing_peer_skipped_subtree_produces_no_descendant_operations_or_snapshot_rows(
) {
    let config = make_run_config(false, 1);
    let trace = OperationTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = TracingOperationExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_recover_directories: vec!["orphans".to_string()],
        fail_recover_peer_ids: Some(vec![1_200_001]),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("no_contributing_skip_source");
    let subordinate_root = next_test_root("no_contributing_skip_subordinate");

    let source = make_peer_session(
        1_200_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "no_contributing_skip_source_db");

    let subordinate = make_peer_session(
        1_200_002,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot =
        prepare_snapshot(&subordinate, "no_contributing_skip_subordinate_db");

    create_directory(&source_root, "orphans");
    create_directory(&subordinate_root, "orphans");
    write_file(
        &source_root,
        "orphans/keeper.txt",
        "source keep in skipped subtree",
    );
    write_file(
        &subordinate_root,
        "orphans/only-subordinate.txt",
        "subordinate only in skipped subtree",
    );

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
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

    assert!(!report.completed);
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "orphans"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed
            )
    }));
    assert!(report.failures.iter().any(|failure| matches!(
        failure,
        kitchensync::sync::SyncFailure::SwapRecovery {
            peer_id,
            directory,
            canon: false,
            ..
        } if *peer_id == 1_200_001 && directory.as_str() == "orphans"
    )));

    assert_eq!(report.traversal.scanned_directories, 2);
    assert_eq!(trace.copy_calls(), 0);
    assert_eq!(trace.displace_calls(), 0);
    assert_eq!(trace.create_calls(), 0);
    assert_eq!(trace.cleanup_calls(), 2);
    assert!(!has_path_or_descendant(&trace.cleanup_paths(), "orphans"));

    let skipped_peer = kitchensync::RelPath::new("orphans/only-subordinate.txt").unwrap();
    assert!(subordinate_snapshot
        .store
        .lookup(&skipped_peer)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&skipped_peer)
        .unwrap()
        .is_none());
    assert!(source_root.join("orphans/keeper.txt").exists());
    assert!(subordinate_root
        .join("orphans/only-subordinate.txt")
        .exists());

    source_snapshot.store.close().unwrap();
    subordinate_snapshot.store.close().unwrap();
}
