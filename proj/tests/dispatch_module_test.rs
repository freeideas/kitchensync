use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use filetime::{set_file_mtime, FileTime};

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

#[derive(Clone)]
struct ControlledOperationExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_displace: Vec<(kitchensync::PeerId, String)>,
    fail_create: Vec<(kitchensync::PeerId, String)>,
}

impl<'a> ControlledOperationExecutor<'a> {
    fn should_fail(
        failures: &[(kitchensync::PeerId, String)],
        peer_id: kitchensync::PeerId,
        path: &kitchensync::RelPath,
    ) -> bool {
        failures.iter().any(|(target_peer, target_path)| {
            *target_peer == peer_id && target_path == path.as_str()
        })
    }
}

impl kitchensync::operations::OperationExecutor for ControlledOperationExecutor<'_> {
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
        if Self::should_fail(&self.fail_displace, peer.id, path) {
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
        if Self::should_fail(&self.fail_create, peer.id, path) {
            return Err(kitchensync::operations::OperationError {
                peer_id: peer.id,
                context: kitchensync::operations::OperationErrorContext::CreateDirectory {
                    path: path.clone(),
                },
                error: kitchensync::TransportError::IoError,
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

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
static TEST_RUN_NONCE: OnceLock<u128> = OnceLock::new();

fn next_test_root(test_name: &str) -> std::path::PathBuf {
    let counter = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut root = std::env::temp_dir();
    let nonce = TEST_RUN_NONCE.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    });
    root.push(format!(
        "kitchensync-dispatch-test-{test_name}-{}-{nonce}-{counter}",
        std::process::id()
    ));

    if root.exists() {
        let metadata = fs::symlink_metadata(&root).unwrap();
        if metadata.is_dir() {
            fs::remove_dir_all(&root).unwrap();
        } else {
            fs::remove_file(&root).unwrap();
        }
    }
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_text(root: &Path, rel: &str, content: &str) {
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

fn has_copy_failures(
    report: &kitchensync::sync::SyncReport,
    peer_id: kitchensync::PeerId,
    path: &str,
) -> bool {
    report.failures.iter().any(|failure| {
        matches!(
            failure,
            kitchensync::sync::SyncFailure::Operation {
                peer_id: failure_peer,
                path: failure_path,
                ..
            } if *failure_peer == peer_id && failure_path.as_str() == path
        )
    })
}

#[test]
fn dispatch_file_outcome_copies_to_missing_destination() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("file_copy_source");
    let destination_root = next_test_root("file_copy_destination");

    let source = make_peer_session(10_001, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "file_copy_source_db");
    let destination = make_peer_session(
        10_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "file_copy_destination_db");

    write_text(&source_root, "docs/readme.txt", "alpha\n");

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
    assert_eq!(report.copies.failed, 0);
    assert!(destination_root.join("docs/readme.txt").is_file());
    assert_eq!(
        fs::read_to_string(destination_root.join("docs/readme.txt")).unwrap(),
        "alpha\n"
    );
}

#[test]
fn dispatch_file_outcome_skips_copy_for_matching_destination_metadata() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("matching_file_source");
    let destination_root = next_test_root("matching_file_destination");

    let source = make_peer_session(10_101, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "matching_file_source_db");
    let destination = make_peer_session(
        10_102,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "matching_file_destination_db");

    write_text(&source_root, "same/unchanged.bin", "bytes\n");
    let source_file = source_root.join("same/unchanged.bin");
    let destination_file = destination_root.join("same/unchanged.bin");
    write_text(&destination_root, "same/unchanged.bin", "bytes\n");
    set_file_times_from_source(&source_file, &destination_file);

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
    assert_eq!(report.copies.succeeded, 0);
    assert_eq!(report.copies.failed, 0);
    assert!(destination_file.is_file());
    assert_eq!(fs::read_to_string(destination_file).unwrap(), "bytes\n");
    assert_eq!(report.traversal.enqueued_copies, 0);
}

#[test]
fn dispatch_file_outcome_replaces_directory_before_copying_file() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("file_dir_conflict_source");
    let destination_root = next_test_root("file_dir_conflict_destination");

    let source = make_peer_session(10_201, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "file_dir_conflict_source_db");
    let destination = make_peer_session(
        10_202,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "file_dir_conflict_destination_db");

    write_text(&source_root, "replace-me", "new");
    fs::create_dir_all(destination_root.join("replace-me")).unwrap();

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
    assert!(destination_root.join("replace-me").is_file());
    assert_eq!(
        fs::read_to_string(destination_root.join("replace-me")).unwrap(),
        "new"
    );
}

#[test]
fn dispatch_displacement_failure_blocks_file_copy() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = ControlledOperationExecutor {
        delegate: &base_executor,
        fail_displace: vec![(10_402, "blocked".to_string())],
        fail_create: Vec::new(),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("blocked_displace_source");
    let destination_root = next_test_root("blocked_displace_destination");

    let source = make_peer_session(10_401, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "blocked_displace_source_db");
    let destination = make_peer_session(
        10_402,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "blocked_displace_destination_db");

    write_text(&source_root, "blocked", "new");
    fs::create_dir_all(destination_root.join("blocked")).unwrap();

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
    assert_eq!(report.copies.succeeded, 0);
    assert!(has_copy_failures(&report, 10_402, "blocked"));
    assert!(destination_root.join("blocked").is_dir());
}

#[test]
fn dispatch_directory_copy_fails_to_recurse_after_create_failure() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = ControlledOperationExecutor {
        delegate: &base_executor,
        fail_displace: Vec::new(),
        fail_create: vec![(11_001, "tree".to_string())],
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("dir_fail_source");
    let destination_root = next_test_root("dir_fail_destination");

    let source = make_peer_session(11_000, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "dir_fail_source_db");
    let destination = make_peer_session(
        11_001,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "dir_fail_destination_db");

    write_text(&source_root, "tree/child.txt", "value");
    write_text(&destination_root, "tree", "old");

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
    assert_eq!(report.copies.succeeded, 0);
    assert!(has_copy_failures(&report, 11_001, "tree"));
    assert!(!destination_root.join("tree/child.txt").exists());
}

#[test]
fn dispatch_directory_outcome_blocks_directory_copy_after_displacement_failure() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = ControlledOperationExecutor {
        delegate: &base_executor,
        fail_displace: vec![(12_001, "tree".to_string())],
        fail_create: Vec::new(),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("dir_displace_fail_source");
    let destination_root = next_test_root("dir_displace_fail_destination");

    let source = make_peer_session(12_000, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "dir_displace_fail_source_db");
    let destination = make_peer_session(
        12_001,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "dir_displace_fail_destination_db");

    write_text(&source_root, "tree/child.txt", "value\n");
    write_text(&destination_root, "tree", "old");

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
    assert_eq!(report.copies.succeeded, 0);
    assert!(has_copy_failures(&report, 12_001, "tree"));
    assert_eq!(report.traversal.enqueued_copies, 0);
    assert!(destination_root.join("tree").is_file());
    assert!(!destination_root.join("tree/child.txt").exists());
}

#[test]
fn dispatch_directory_recurse_after_creating_directory() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("dir_recurse_source");
    let destination_root = next_test_root("dir_recurse_destination");

    let source = make_peer_session(11_101, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "dir_recurse_source_db");
    let destination = make_peer_session(
        11_102,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "dir_recurse_destination_db");

    write_text(&source_root, "tree/child.txt", "value\n");
    write_text(&destination_root, "tree", "old");

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
    assert!(destination_root.join("tree").is_dir());
    assert!(destination_root.join("tree/child.txt").is_file());
    assert_eq!(
        fs::read_to_string(destination_root.join("tree/child.txt")).unwrap(),
        "value\n"
    );
}

#[test]
fn dispatch_absence_outcome_displaces_existing_entry() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("absence_source");
    let destination_root = next_test_root("absence_destination");

    let source = make_peer_session(11_201, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "absence_source_db");
    let destination = make_peer_session(
        11_202,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "absence_destination_db");

    write_text(&destination_root, "vanish.txt", "gone soon");

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
    assert_eq!(report.copies.failed, 0);
    assert!(!destination_root.join("vanish.txt").exists());
}

#[test]
fn dispatch_file_copy_in_dry_run_does_not_mutate_destination() {
    let config = make_run_config(true, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("dry_run_copy_source");
    let destination_root = next_test_root("dry_run_copy_destination");

    let source = make_peer_session(11_301, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "dry_run_copy_source_db");
    let destination = make_peer_session(
        11_302,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "dry_run_copy_destination_db");

    write_text(&source_root, "notes/dry.txt", "preview\n");

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
    assert!(!destination_root.join("notes/dry.txt").exists());
}

#[test]
fn dispatch_absence_outcome_displaces_subordinate_live_entry() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("absence_subordinate_source");
    let destination_root = next_test_root("absence_subordinate_destination");
    let subordinate_root = next_test_root("absence_subordinate_subordinate");

    let source = make_peer_session(11_401, &source_root, kitchensync::EffectivePeerRole::Canon);
    let mut source_snapshot = prepare_snapshot(&source, "absence_subordinate_source_db");
    let destination = make_peer_session(
        11_402,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "absence_subordinate_destination_db");
    let subordinate = make_peer_session(
        11_403,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot =
        prepare_snapshot(&subordinate, "absence_subordinate_subordinate_db");

    write_text(&subordinate_root, "vanish.txt", "target-only stale value");

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
    assert_eq!(report.copies.failed, 0);
    assert!(!subordinate_root.join("vanish.txt").exists());
    assert!(!has_copy_failures(&report, 11_403, "vanish.txt"));
}

fn set_file_times_from_source(source: &Path, destination: &Path) {
    let metadata = source.metadata().unwrap();
    let timestamp = FileTime::from_last_modification_time(&metadata);
    set_file_mtime(destination, timestamp).unwrap();
}
