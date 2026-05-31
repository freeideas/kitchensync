use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default, Clone)]
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

struct FailingRecoveryExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_recover_directories: Vec<String>,
    fail_peer_ids: Option<Vec<kitchensync::PeerId>>,
}

impl<'a> kitchensync::operations::OperationExecutor for FailingRecoveryExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        let peer_matches = match self.fail_peer_ids.as_ref() {
            Some(ids) => ids.contains(&peer.id),
            None => true,
        };

        if peer_matches
            && self
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

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let normalized = test_name.replace(['\\', '/'], "_");
    let mut path = std::env::temp_dir();
    path.push(format!("kitchensync-traversal-resp5-{normalized}-{seq}"));

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
fn sync_run_counts_scanned_directories_for_nested_path_traversal() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("counted_scan_source");
    let destination_root = next_test_root("counted_scan_destination");

    let source = make_peer_session(
        101_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "counted_scan_source_db");
    let destination = make_peer_session(
        101_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "counted_scan_destination_db");

    fs::create_dir_all(source_root.join("tree")).unwrap();
    fs::create_dir_all(source_root.join("tree").join("leaf")).unwrap();
    write_file(
        &source_root,
        "tree/leaf/notes.txt",
        "this file should be synced",
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
    assert_eq!(report.traversal.scanned_directories, 3);
}

#[test]
fn sync_run_records_no_contributing_skip_as_skipped_subtree() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingRecoveryExecutor {
        delegate: &base_executor,
        fail_recover_directories: vec!["shared".to_string()],
        fail_peer_ids: None,
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("no_contributing_skip_source");
    let destination_root = next_test_root("no_contributing_skip_destination");

    let source = make_peer_session(
        102_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "no_contributing_skip_source_db");
    let destination = make_peer_session(
        102_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "no_contributing_skip_destination_db");

    fs::create_dir_all(source_root.join("shared").join("inner")).unwrap();
    fs::create_dir_all(destination_root.join("shared").join("inner")).unwrap();
    write_file(&source_root, "shared/inner/guard.txt", "skip payload");

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
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "shared"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed
            )
    }));
}

#[test]
fn sync_run_records_canon_failure_skipped_subtree_without_descending() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = FailingRecoveryExecutor {
        delegate: &base_executor,
        fail_recover_directories: vec!["skipme".to_string()],
        fail_peer_ids: Some(vec![120_001]),
    };
    let scheduler = make_scheduler();

    let canon_root = next_test_root("canon_fail_source");
    let subordinate_root = next_test_root("canon_fail_subordinate");

    let canon = make_peer_session(120_001, &canon_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "canon_fail_canon_db");
    let subordinate = make_peer_session(
        120_002,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot = prepare_snapshot(&subordinate, "canon_fail_subordinate_db");

    fs::create_dir_all(canon_root.join("keep")).unwrap();
    fs::create_dir_all(canon_root.join("keep").join("nested")).unwrap();
    write_file(&canon_root, "keep/nested/kept.txt", "copied onward");

    fs::create_dir_all(canon_root.join("skipme").join("forbidden")).unwrap();
    fs::create_dir_all(subordinate_root.join("skipme")).unwrap();

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
    assert_eq!(report.traversal.scanned_directories, 4);
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "skipme"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::CanonListingUnavailable { peer_id }
                    if peer_id == 120_001
            )
    }));
}
