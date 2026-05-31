use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

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

struct ControlledOperationExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_copy: Vec<(kitchensync::PeerId, String)>,
    fail_create: Vec<(kitchensync::PeerId, String)>,
    fail_displace: Vec<(kitchensync::PeerId, String)>,
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
        if Self::should_fail(&self.fail_copy, destination_peer.id, destination_path) {
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

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> std::path::PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    let normalized = test_name.replace(['\\', '/'], "_");
    path.push(format!("kitchensync_snapshot_flow_test_{normalized}_{seq}"));

    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }

    fs::create_dir_all(&path).unwrap();
    path
}

// Not reasonably testable through `kitchensync::sync::run` alone:
// - unreachable-peer and listing-fail subtree skip effects are applied before mutation hooks run,
// - direct store write failures, cleanup request failures, and cleanup timing/completion in
//   `snapshot_flow` are not observable with
//   this public harness.

fn write_file(root: &std::path::Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn make_peer_url(root: &std::path::Path) -> kitchensync::PeerUrl {
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
    root: &std::path::Path,
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

fn make_run_config() -> kitchensync::RunConfig {
    make_run_config_with_excludes(Vec::new())
}

fn make_run_config_with_excludes(excludes: Vec<kitchensync::RelPath>) -> kitchensync::RunConfig {
    kitchensync::RunConfig {
        dry_run: false,
        max_copies: 2,
        retries_copy: 1,
        retries_list: 1,
        timeout_conn: 1,
        timeout_idle: 1,
        verbosity: kitchensync::Verbosity::Error,
        keep_tmp_days: 1,
        keep_bak_days: 1,
        keep_del_days: 1,
        excludes,
    }
}

fn make_scheduler(config: &kitchensync::RunConfig) -> kitchensync::runtime::CopyScheduler {
    kitchensync::runtime::CopyScheduler::new(
        kitchensync::runtime::SchedulerConfig {
            max_copies: config.max_copies,
            retries_copy: config.retries_copy,
        },
        NullSink,
        NullSink,
    )
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

fn metadata(name: &str, kind: kitchensync::EntryKind, byte_size: i64) -> kitchensync::EntryMeta {
    kitchensync::EntryMeta {
        name: name.to_string(),
        kind,
        mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        byte_size,
    }
}

#[test]
fn snapshot_flow_records_confirmed_present_file_and_directory_rows() {
    let config = make_run_config();
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("confirmed_present_source");
    let destination_root = next_test_root("confirmed_present_destination");

    let source = make_peer_session(
        11_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "confirmed_present_source_db");

    let destination = make_peer_session(
        11_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "confirmed_present_destination_db");

    write_file(&source_root, "readme.txt", "hello");
    fs::create_dir_all(source_root.join("notes")).unwrap();
    write_file(&source_root, "notes/guide.txt", "body");

    let path_file = kitchensync::RelPath::new("readme.txt").unwrap();
    let path_dir = kitchensync::RelPath::new("notes").unwrap();

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

    let source_file = source_snapshot.store.lookup(&path_file).unwrap().unwrap();
    let source_dir = source_snapshot.store.lookup(&path_dir).unwrap().unwrap();
    let dest_file = destination_snapshot
        .store
        .lookup(&path_file)
        .unwrap()
        .unwrap();
    let dest_dir = destination_snapshot
        .store
        .lookup(&path_dir)
        .unwrap()
        .unwrap();

    assert_eq!(
        source_file.kind,
        kitchensync::snapshot::SnapshotEntryKind::File
    );
    assert_eq!(source_file.byte_size, 5);
    assert_eq!(source_file.deleted_time, None);
    assert_eq!(source_file.last_seen.is_some(), true);

    assert_eq!(
        source_dir.kind,
        kitchensync::snapshot::SnapshotEntryKind::Directory
    );
    assert_eq!(source_dir.byte_size, -1);

    assert_eq!(
        dest_file.kind,
        kitchensync::snapshot::SnapshotEntryKind::File
    );
    assert_eq!(dest_file.byte_size, 5);
    assert_eq!(dest_file.deleted_time, None);
    assert_eq!(dest_file.last_seen.is_some(), true);

    assert_eq!(
        dest_dir.kind,
        kitchensync::snapshot::SnapshotEntryKind::Directory
    );
    assert_eq!(dest_dir.byte_size, -1);
    assert_eq!(dest_dir.last_seen.is_some(), true);
}

#[test]
fn snapshot_flow_keeps_destination_last_seen_when_copy_fails_with_existing_row() {
    let config = make_run_config();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = ControlledOperationExecutor {
        delegate: &base_executor,
        fail_copy: vec![(11_102, "alpha.txt".to_string())],
        fail_create: Vec::new(),
        fail_displace: Vec::new(),
    };
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("copy_fail_existing_source");
    let destination_root = next_test_root("copy_fail_existing_destination");

    let source = make_peer_session(
        11_101,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "copy_fail_existing_source_db");
    let destination = make_peer_session(
        11_102,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "copy_fail_existing_destination_db");

    let path = kitchensync::RelPath::new("alpha.txt").unwrap();

    write_file(&source_root, path.as_str(), "new content");
    let winning_mod_time = kitchensync::Timestamp("2099-01-02_00-00-00_000000Z".to_string());
    source
        .transport
        .set_mod_time(&path, winning_mod_time.clone())
        .unwrap();

    let prior_meta = metadata("alpha.txt", kitchensync::EntryKind::File, 11);
    let prior_seen = destination_snapshot
        .store
        .upsert_confirmed_present(&path, &prior_meta)
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
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(failure, kitchensync::sync::SyncFailure::Copy { result } if result.destination_path == path)));

    let after = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after.byte_size, 11);
    assert_eq!(after.mod_time, winning_mod_time);
    assert_eq!(after.last_seen, Some(prior_seen));
    assert_eq!(after.deleted_time, None);
}

#[test]
fn snapshot_flow_advances_last_seen_after_successful_copy() {
    let config = make_run_config();
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("copy_success_source");
    let destination_root = next_test_root("copy_success_destination");

    let source = make_peer_session(
        11_201,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "copy_success_source_db");
    let destination = make_peer_session(
        11_202,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "copy_success_destination_db");

    let path = kitchensync::RelPath::new("winner.txt").unwrap();

    write_file(&source_root, path.as_str(), "newer");
    write_file(&destination_root, path.as_str(), "old");

    let before_meta = metadata(path.as_str(), kitchensync::EntryKind::File, 3);
    let before_last_seen = destination_snapshot
        .store
        .upsert_confirmed_present(&path, &before_meta)
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

    let after = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after.byte_size, 5);
    assert_eq!(after.deleted_time, None);
    assert!(after.last_seen.as_ref().unwrap().0 > before_last_seen.0);
}

#[test]
fn snapshot_flow_keeps_null_last_seen_when_copy_fails_without_prior_destination_row() {
    let config = make_run_config();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = ControlledOperationExecutor {
        delegate: &base_executor,
        fail_copy: vec![(11_302, "orphan.txt".to_string())],
        fail_create: Vec::new(),
        fail_displace: Vec::new(),
    };
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("copy_fail_without_existing_row_source");
    let destination_root = next_test_root("copy_fail_without_existing_row_destination");

    let source = make_peer_session(
        11_301,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "copy_fail_without_existing_row_source_db");
    let destination = make_peer_session(
        11_302,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(
        &destination,
        "copy_fail_without_existing_row_destination_db",
    );

    let path = kitchensync::RelPath::new("orphan.txt").unwrap();
    write_file(&source_root, path.as_str(), "copy");

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
    assert!(!report.failures.is_empty());

    let row = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, None);
    assert_eq!(row.deleted_time, None);
    assert_eq!(row.byte_size, 4);
}

#[test]
fn snapshot_flow_marks_preexisting_row_as_absent_when_absence_wins() {
    let config = make_run_config();
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("absence_non_tombstoned_source");
    let destination_root = next_test_root("absence_non_tombstoned_destination");

    let source = make_peer_session(
        11_901,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "absence_non_tombstoned_source_db");
    let destination = make_peer_session(
        11_902,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "absence_non_tombstoned_destination_db");

    let path = kitchensync::RelPath::new("ghost.txt").unwrap();
    write_file(&source_root, path.as_str(), "present");
    source
        .transport
        .set_mod_time(
            &path,
            kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        )
        .unwrap();

    let existing_meta = metadata("ghost.txt", kitchensync::EntryKind::File, 7);
    let prior_seen = destination_snapshot
        .store
        .upsert_confirmed_present(&path, &existing_meta)
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
    assert_eq!(report.traversal.decided_entries, 1);
    assert_eq!(report.copies.succeeded, 0);

    let after = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after.last_seen, Some(prior_seen.clone()));
    assert_eq!(after.deleted_time, Some(prior_seen));
}

#[test]
fn snapshot_flow_keeps_tombstoned_absence_row_unchanged() {
    let config = make_run_config();
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("absence_tombstone_source");
    let destination_root = next_test_root("absence_tombstone_destination");

    let source = make_peer_session(
        11_903,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "absence_tombstone_source_db");
    let destination = make_peer_session(
        11_904,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "absence_tombstone_destination_db");

    let path = kitchensync::RelPath::new("retired.txt").unwrap();
    write_file(&source_root, path.as_str(), "present");
    source
        .transport
        .set_mod_time(
            &path,
            kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        )
        .unwrap();

    let existing_meta = metadata("retired.txt", kitchensync::EntryKind::File, 8);
    destination_snapshot
        .store
        .upsert_confirmed_present(&path, &existing_meta)
        .unwrap();
    destination_snapshot.store.mark_absent(&path).unwrap();
    let before = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    let before_last_seen = before.last_seen.clone();
    let before_deleted = before.deleted_time.clone();

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
    assert_eq!(report.copies.succeeded, 0);

    let after = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after.last_seen, before_last_seen);
    assert_eq!(after.deleted_time, before_deleted);
}

#[test]
fn snapshot_flow_records_directory_creation_as_present() {
    let config = make_run_config();
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("directory_create_source");
    let destination_root = next_test_root("directory_create_destination");

    let source = make_peer_session(
        11_401,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "directory_create_source_db");
    let destination = make_peer_session(
        11_402,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "directory_create_destination_db");

    fs::create_dir_all(source_root.join("created")).unwrap();

    let path = kitchensync::RelPath::new("created").unwrap();

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
    assert!(destination_root.join("created").is_dir());

    let row = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(
        row.kind,
        kitchensync::snapshot::SnapshotEntryKind::Directory
    );
    assert_eq!(row.byte_size, -1);
    assert_eq!(row.deleted_time, None);
    assert!(row.last_seen.is_some());
}

#[test]
fn snapshot_flow_preserves_row_on_directory_create_failure() {
    let config = make_run_config();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = ControlledOperationExecutor {
        delegate: &base_executor,
        fail_copy: Vec::new(),
        fail_create: vec![(11_503, "blocked".to_string())],
        fail_displace: Vec::new(),
    };
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("directory_create_failure_source");
    let destination_root = next_test_root("directory_create_failure_destination");

    let source = make_peer_session(
        11_501,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "directory_create_failure_source_db");
    let destination = make_peer_session(
        11_503,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "directory_create_failure_destination_db");

    fs::create_dir_all(source_root.join("blocked")).unwrap();

    let path = kitchensync::RelPath::new("blocked").unwrap();
    let existing_meta = metadata("blocked", kitchensync::EntryKind::Directory, -1);
    let existing_seen = destination_snapshot
        .store
        .upsert_confirmed_present(&path, &existing_meta)
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
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(failure, kitchensync::sync::SyncFailure::Operation { path: value, .. } if *value == path)));

    let row = destination_snapshot.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, Some(existing_seen));
    assert_eq!(row.mod_time, existing_meta.mod_time);
    assert_eq!(row.byte_size, -1);
    assert_eq!(row.deleted_time, None);
    assert!(!destination_root.join(path.as_str()).exists());
}

#[test]
fn snapshot_flow_cascades_displaced_directory_on_successful_displacement_peer() {
    let config = make_run_config();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let executor = ControlledOperationExecutor {
        delegate: &base_executor,
        fail_copy: Vec::new(),
        fail_create: Vec::new(),
        fail_displace: vec![(11_603, "conflict".to_string())],
    };
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("displaced_directory_source");
    let destination1_root = next_test_root("displaced_directory_destination_1");
    let destination2_root = next_test_root("displaced_directory_destination_2");

    let source = make_peer_session(
        11_601,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "displaced_directory_source_db");

    let destination1 = make_peer_session(
        11_602,
        &destination1_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination1_snapshot =
        prepare_snapshot(&destination1, "displaced_directory_destination_1_db");
    let destination2 = make_peer_session(
        11_603,
        &destination2_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination2_snapshot =
        prepare_snapshot(&destination2, "displaced_directory_destination_2_db");

    write_file(&source_root, "conflict", "winner");
    fs::create_dir_all(destination1_root.join("conflict")).unwrap();
    write_file(&destination1_root, "conflict/child.txt", "first");
    fs::create_dir_all(destination2_root.join("conflict")).unwrap();
    write_file(&destination2_root, "conflict/child.txt", "second");

    let path = kitchensync::RelPath::new("conflict").unwrap();
    let child = kitchensync::RelPath::new("conflict/child.txt").unwrap();

    let root_meta = metadata("conflict", kitchensync::EntryKind::Directory, -1);
    let child_meta = metadata("child.txt", kitchensync::EntryKind::File, 5);

    destination1_snapshot
        .store
        .upsert_confirmed_present(&path, &root_meta)
        .unwrap();
    destination1_snapshot
        .store
        .upsert_confirmed_present(&child, &child_meta)
        .unwrap();
    let destination1_root_before = destination1_snapshot.store.lookup(&path).unwrap().unwrap();

    destination2_snapshot
        .store
        .upsert_confirmed_present(&path, &root_meta)
        .unwrap();
    destination2_snapshot
        .store
        .upsert_confirmed_present(&child, &child_meta)
        .unwrap();
    let destination2_root_before = destination2_snapshot.store.lookup(&path).unwrap().unwrap();
    let destination2_child_before = destination2_snapshot.store.lookup(&child).unwrap().unwrap();

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &source,
            snapshot: &mut source_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination1,
            snapshot: &mut destination1_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &destination2,
            snapshot: &mut destination2_snapshot.store,
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
        kitchensync::sync::SyncFailure::Operation { peer_id: 11_603, path: value, .. }
        if *value == path
    )));

    let destination1_after = destination1_snapshot.store.lookup(&path).unwrap().unwrap();
    let destination1_child_after = destination1_snapshot.store.lookup(&child).unwrap().unwrap();
    let destination2_after = destination2_snapshot.store.lookup(&path).unwrap().unwrap();
    let destination2_child_after = destination2_snapshot.store.lookup(&child).unwrap().unwrap();

    assert_eq!(
        destination1_child_after.deleted_time,
        destination1_root_before.last_seen.clone()
    );
    assert_eq!(destination1_after.byte_size, 6);
    assert!(destination1_after.deleted_time.is_none());
    assert!(
        destination1_after.last_seen.as_ref().unwrap().0
            > destination1_root_before
                .last_seen
                .as_ref()
                .expect("destination1 root has a prior last_seen")
                .0
    );

    assert_eq!(
        destination2_after.last_seen,
        Some(destination2_root_before.last_seen.unwrap())
    );
    assert_eq!(destination2_after.deleted_time, None);
    assert_eq!(
        destination2_after.kind,
        kitchensync::snapshot::SnapshotEntryKind::Directory
    );
    assert_eq!(destination2_after.byte_size, -1);
    assert_eq!(destination2_child_after.deleted_time, None);
    assert_eq!(
        destination2_child_after.kind,
        kitchensync::snapshot::SnapshotEntryKind::File
    );
    assert_eq!(
        destination2_child_after.byte_size,
        destination2_child_before.byte_size
    );
}

#[test]
fn snapshot_flow_does_not_modify_excluded_paths() {
    let config = make_run_config_with_excludes(vec![
        kitchensync::RelPath::new("cache/temp.txt").unwrap(),
        kitchensync::RelPath::new("skip").unwrap(),
    ]);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler(&config);

    let source_root = next_test_root("excluded_paths_source");
    let destination_root = next_test_root("excluded_paths_destination");

    let source = make_peer_session(
        11_701,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "excluded_paths_source_db");
    let destination = make_peer_session(
        11_702,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "excluded_paths_destination_db");

    write_file(&source_root, "keep.txt", "keep");
    write_file(&source_root, "cache/temp.txt", "ignore");
    fs::create_dir_all(source_root.join("skip")).unwrap();
    write_file(&source_root, "skip/child.txt", "ignoreme");

    let keep = kitchensync::RelPath::new("keep.txt").unwrap();
    let excluded_file = kitchensync::RelPath::new("cache/temp.txt").unwrap();
    let excluded_dir = kitchensync::RelPath::new("skip").unwrap();

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
    assert!(destination_root.join("keep.txt").exists());
    assert!(!destination_root.join("cache/temp.txt").exists());
    assert!(!destination_root.join("skip").exists());
    assert!(destination_snapshot.store.lookup(&keep).unwrap().is_some());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_file)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_dir)
        .unwrap()
        .is_none());
}
