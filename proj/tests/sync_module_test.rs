use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};

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
        self.events
            .lock()
            .expect("progress sink poisoned")
            .clone()
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
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport> {
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
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(failure, kitchensync::sync::SyncFailure::InvalidRunInput { reason: kitchensync::sync::SyncInputError::EmptyPeerSet } )));
}

#[test]
fn sync_run_rejects_snapshot_peer_mismatch() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("snapshot_mismatch_source");
    let wrong_root = next_test_root("snapshot_mismatch_wrong");

    let source = make_peer_session(10_001, &source_root, kitchensync::EffectivePeerRole::Contributing);
    let mut source_snapshot = prepare_snapshot(&source, "snapshot_mismatch_source_db");
    let wrong = make_peer_session(10_002, &wrong_root, kitchensync::EffectivePeerRole::Contributing);
    let mut wrong_snapshot = prepare_snapshot(&wrong, "snapshot_mismatch_wrong_db");

    let mut peers = [kitchensync::sync::SyncPeer {
        session: &source,
        snapshot: &mut source_snapshot.store,
    }, kitchensync::sync::SyncPeer {
        session: &wrong,
        snapshot: &mut wrong_snapshot.store,
    }];

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
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(
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
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(
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
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(
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

    assert_eq!(first_scan.unwrap().as_str(), "");
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
    assert!(source_snapshot.store.lookup(&excluded_file).unwrap().is_none());
    assert!(destination_snapshot.store.lookup(&excluded_dir).unwrap().is_none());
    assert!(destination_snapshot.store.lookup(&excluded_file).unwrap().is_none());
}

#[test]
fn sync_run_prefers_file_when_file_and_directory_conflict_has_no_canon() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let file_root = next_test_root("conflict_file_root");
    let dir_root = next_test_root("conflict_dir_root");

    let file_peer = make_peer_session(60_001, &file_root, kitchensync::EffectivePeerRole::Contributing);
    let mut file_snapshot = prepare_snapshot(&file_peer, "conflict_file_root_db");
    let dir_peer = make_peer_session(60_002, &dir_root, kitchensync::EffectivePeerRole::Contributing);
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
    assert_eq!(fs::read_to_string(dir_root.join("entry")).unwrap(), "winner-data");
    assert!(file_root.join("entry").is_file());
    assert!(file_snapshot
        .store
        .lookup(&shared_path)
        .unwrap()
        .is_some());
}

#[test]
fn sync_run_copies_winner_of_file_competition_to_smaller_file_peer() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("winner_source");
    let destination_root = next_test_root("winner_destination");

    let source = make_peer_session(70_001, &source_root, kitchensync::EffectivePeerRole::Contributing);
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
    assert_eq!(fs::read_to_string(destination_root.join("payload.bin")).unwrap(), "short");
    assert!(destination_snapshot.store.lookup(&path).unwrap().is_some());
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
    let mut destination_snapshot = prepare_snapshot(
        &shared_destination,
        "failshared_destination_db",
    );

    fs::create_dir_all(shared_source_root.join("shared"));
    fs::create_dir_all(shared_destination_root.join("shared"));

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
        .any(|entry| entry.directory.as_str() == "shared" && matches!(entry.reason, kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed)));
    assert!(report
        .failures
        .iter()
        .any(|failure| matches!(
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

    let source = make_peer_session(90_001, &source_root, kitchensync::EffectivePeerRole::Contributing);
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
