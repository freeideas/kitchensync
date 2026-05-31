use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
struct NullSink;

impl kitchensync::runtime::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::runtime::DiagnosticEvent) {}
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

#[derive(Clone, Default)]
struct OperationTrace {
    cleanup: Arc<Mutex<Vec<String>>>,
    create_dir: Arc<Mutex<Vec<String>>>,
    displace: Arc<Mutex<Vec<String>>>,
}

impl OperationTrace {
    fn record_cleanup(&self, directory: &kitchensync::RelPath) {
        self.cleanup
            .lock()
            .expect("cleanup trace lock poisoned")
            .push(directory.as_str().to_string());
    }

    fn record_create_dir(&self, path: &kitchensync::RelPath) {
        self.create_dir
            .lock()
            .expect("create trace lock poisoned")
            .push(path.as_str().to_string());
    }

    fn record_displace(&self, path: &kitchensync::RelPath) {
        self.displace
            .lock()
            .expect("displace trace lock poisoned")
            .push(path.as_str().to_string());
    }

    fn has_activity_under(&self, prefix: &str) -> bool {
        let nested = format!("{}/", prefix);
        let cleanup = self.cleanup.lock().expect("cleanup trace lock poisoned");
        let create_dir = self.create_dir.lock().expect("create trace lock poisoned");
        let displace = self.displace.lock().expect("displace trace lock poisoned");

        cleanup
            .iter()
            .chain(create_dir.iter())
            .chain(displace.iter())
            .any(|path| path == prefix || path.starts_with(&nested))
    }

    fn any_cleanup_under(&self, prefix: &str) -> bool {
        let nested = format!("{}/", prefix);
        self.cleanup
            .lock()
            .expect("cleanup trace lock poisoned")
            .iter()
            .any(|path| path == prefix || path.starts_with(&nested))
    }
}

struct FailingRecoveryExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    trace: OperationTrace,
    fail_recover_directories: Vec<String>,
    fail_recover_peer_ids: Option<Vec<kitchensync::PeerId>>,
}

impl<'a> FailingRecoveryExecutor<'a> {
    fn should_fail(
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

impl<'a> kitchensync::operations::OperationExecutor for FailingRecoveryExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        if self.should_fail(peer, directory) {
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
        self.trace.record_displace(path);
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.trace.record_create_dir(path);
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
    path.push(format!("kitchensync-traversal-errors-2-{normalized}-{seq}"));

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
fn traversal_2_excludes_do_not_trigger_downstream_decisions_operations_or_snapshot_updates() {
    let config = kitchensync::RunConfig {
        excludes: vec![
            kitchensync::RelPath::new("skip.txt").unwrap(),
            kitchensync::RelPath::new("skipdir").unwrap(),
        ],
        ..make_run_config(false, 1)
    };

    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("exclude_paths_source");
    let destination_root = next_test_root("exclude_paths_destination");

    let source = make_peer_session(
        210_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "exclude_paths_source_db");

    let destination = make_peer_session(
        210_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "exclude_paths_destination_db");

    write_file(&source_root, "keep.txt", "should sync");
    write_file(&source_root, "skip.txt", "should never sync");
    write_file(&source_root, "skipdir/inner.txt", "nested exclude");

    write_file(&destination_root, "skip.txt", "preexisting keep");
    write_file(&destination_root, "skipdir/inner.txt", "preexisting nested");

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
    let skipped_file = kitchensync::RelPath::new("skip.txt").unwrap();
    let skipped_nested = kitchensync::RelPath::new("skipdir/inner.txt").unwrap();

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);
    assert_eq!(report.traversal.enqueued_copies, 1);
    assert_eq!(
        fs::read_to_string(destination_root.join("keep.txt")).unwrap(),
        "should sync"
    );
    assert_eq!(
        fs::read_to_string(destination_root.join("skip.txt")).unwrap(),
        "preexisting keep"
    );
    assert_eq!(
        fs::read_to_string(destination_root.join("skipdir/inner.txt")).unwrap(),
        "preexisting nested"
    );

    assert!(source_snapshot.store.lookup(&keep).unwrap().is_some());
    assert!(destination_snapshot.store.lookup(&keep).unwrap().is_some());
    assert!(source_snapshot
        .store
        .lookup(&skipped_file)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&skipped_file)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&skipped_nested)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&skipped_nested)
        .unwrap()
        .is_none());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn traversal_2_canon_recovery_skip_prevents_child_processing_and_cleanup() {
    let config = make_run_config(false, 1);
    let trace = OperationTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingRecoveryExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_recover_directories: vec!["blocked".to_string()],
        fail_recover_peer_ids: Some(vec![220_001]),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("canon_skip_root_source");
    let destination_root = next_test_root("canon_skip_root_destination");

    let canon = make_peer_session(220_001, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "canon_skip_root_source_db");

    let subordinate = make_peer_session(
        220_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot = prepare_snapshot(&subordinate, "canon_skip_root_destination_db");

    fs::create_dir_all(source_root.join("blocked")).unwrap();
    fs::create_dir_all(destination_root.join("blocked")).unwrap();
    write_file(
        &source_root,
        "blocked/nested.txt",
        "should remain untouched",
    );
    write_file(
        &destination_root,
        "blocked/nested.txt",
        "destination preexisting",
    );

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &canon,
            snapshot: &mut canon_snapshot.store,
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
    assert_eq!(report.traversal.scanned_directories, 2);
    assert_eq!(report.traversal.enqueued_copies, 0);
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "blocked"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::CanonListingUnavailable { .. }
            )
    }));
    assert!(report.failures.iter().any(|failure| {
        matches!(
            failure,
            kitchensync::sync::SyncFailure::SwapRecovery {
                canon: true,
                directory,
                ..
            } if directory.as_str() == "blocked"
        )
    }));
    assert!(!trace.has_activity_under("blocked"));
    assert!(!trace.any_cleanup_under("blocked"));

    let nested = kitchensync::RelPath::new("blocked/nested.txt").unwrap();
    assert!(canon_snapshot.store.lookup(&nested).unwrap().is_none());
    assert!(subordinate_snapshot
        .store
        .lookup(&nested)
        .unwrap()
        .is_none());
    assert_eq!(
        fs::read_to_string(destination_root.join("blocked/nested.txt")).unwrap(),
        "destination preexisting"
    );

    canon_snapshot.store.close().unwrap();
    subordinate_snapshot.store.close().unwrap();
}

#[test]
fn traversal_2_no_contributing_subtree_skip_prevents_child_processing_and_cleanup() {
    let config = make_run_config(false, 1);
    let trace = OperationTrace::default();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingRecoveryExecutor {
        delegate: &base_executor,
        trace: trace.clone(),
        fail_recover_directories: vec!["stopped".to_string()],
        fail_recover_peer_ids: None,
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("no_contrib_skip_source");
    let destination_root = next_test_root("no_contrib_skip_destination");

    let source = make_peer_session(
        230_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "no_contrib_skip_source_db");

    let destination = make_peer_session(
        230_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "no_contrib_skip_destination_db");

    fs::create_dir_all(source_root.join("stopped")).unwrap();
    fs::create_dir_all(destination_root.join("stopped")).unwrap();
    write_file(&source_root, "stopped/inner.txt", "should remain untouched");

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
    assert_eq!(report.traversal.scanned_directories, 2);
    assert_eq!(report.traversal.enqueued_copies, 0);
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "stopped"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed
            )
    }));
    assert!(report.failures.iter().any(|failure| {
        matches!(
            failure,
            kitchensync::sync::SyncFailure::SwapRecovery {
                canon: false,
                directory,
                ..
            } if directory.as_str() == "stopped"
        )
    }));
    assert!(!trace.has_activity_under("stopped"));
    assert!(!trace.any_cleanup_under("stopped"));

    let nested = kitchensync::RelPath::new("stopped/inner.txt").unwrap();
    assert!(source_snapshot.store.lookup(&nested).unwrap().is_none());
    assert!(destination_snapshot
        .store
        .lookup(&nested)
        .unwrap()
        .is_none());
    assert!(!destination_root.join("stopped/inner.txt").exists());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}
