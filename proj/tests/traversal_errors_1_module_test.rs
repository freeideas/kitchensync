use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
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

#[derive(Default, Clone)]
struct RecordingDiagnosticSink {
    events: Arc<Mutex<Vec<kitchensync::DiagnosticEvent>>>,
}

impl kitchensync::runtime::DiagnosticSink for RecordingDiagnosticSink {
    fn publish(&self, event: kitchensync::DiagnosticEvent) {
        self.events
            .lock()
            .expect("diagnostic lock poisoned")
            .push(event);
    }
}

impl kitchensync::DiagnosticSink for RecordingDiagnosticSink {
    fn publish(&self, event: kitchensync::DiagnosticEvent) {
        self.events
            .lock()
            .expect("diagnostic lock poisoned")
            .push(event);
    }
}

impl RecordingDiagnosticSink {
    fn has_error(&self) -> bool {
        self.events
            .lock()
            .expect("diagnostic lock poisoned")
            .iter()
            .any(|event| matches!(event, kitchensync::DiagnosticEvent::Error { .. }))
    }
}

struct FailingListingTransport {
    delegate: kitchensync::TransportHandle,
    fail_directory: String,
}

impl kitchensync::TransportBackend for FailingListingTransport {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        if path.as_str() == self.fail_directory {
            return Err(kitchensync::TransportError::IoError);
        }
        self.delegate.list_dir(path)
    }

    fn stat(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::EntryMeta, kitchensync::TransportError> {
        self.delegate.stat(path)
    }

    fn open_read(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportRead, kitchensync::TransportError> {
        self.delegate.open_read(path)
    }

    fn open_write(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportWrite, kitchensync::TransportError> {
        self.delegate.open_write(path)
    }

    fn rename_no_overwrite(
        &self,
        src: &kitchensync::RelPath,
        dst: &kitchensync::RelPath,
    ) -> Result<(), kitchensync::TransportError> {
        self.delegate.rename_no_overwrite(src, dst)
    }

    fn delete_file(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.delegate.delete_file(path)
    }

    fn create_dir(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.delegate.create_dir(path)
    }

    fn delete_dir(&self, path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.delegate.delete_dir(path)
    }

    fn set_mod_time(
        &self,
        path: &kitchensync::RelPath,
        time: kitchensync::Timestamp,
    ) -> Result<(), kitchensync::TransportError> {
        self.delegate.set_mod_time(path, time)
    }
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
    path.push(format!("kitchensync-traversal-errors-1-{normalized}-{seq}"));

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
fn sync_run_reports_listing_failure_and_keeps_failed_subtree_file_unchanged() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();
    let diagnostics = RecordingDiagnosticSink::default();

    let canon_root = next_test_root("listing_failure_canon");
    let listing_root = next_test_root("listing_failure_sharing");

    let canon = make_peer_session(110_001, &canon_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "listing_failure_canon_db");

    let listing_peer_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&listing_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();
    let failing_peer_transport = FailingListingTransport {
        delegate: listing_peer_transport,
        fail_directory: "shared".to_string(),
    };
    let failing_peer = make_peer_session_with_transport(
        110_002,
        &listing_root,
        kitchensync::EffectivePeerRole::Contributing,
        failing_peer_transport,
    );
    let mut failing_snapshot = prepare_snapshot(&failing_peer, "listing_failure_sharing_db");

    write_file(&canon_root, "shared/file.txt", "from canon");
    write_file(&listing_root, "shared/file.txt", "from failing peer");

    let target = kitchensync::RelPath::new("shared/file.txt").unwrap();
    assert!(failing_snapshot.store.lookup(&target).unwrap().is_none());

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &canon,
            snapshot: &mut canon_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &failing_peer,
            snapshot: &mut failing_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &diagnostics,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(diagnostics.has_error());
    assert!(report.failures.iter().any(|failure| {
        matches!(
            failure,
            kitchensync::sync::SyncFailure::Listing {
                peer_id: 110_002,
                directory,
                canon: false,
                ..
            } if directory.as_str() == "shared"
        )
    }));
    assert_eq!(
        read_file(&listing_root, "shared/file.txt"),
        "from failing peer"
    );
    assert!(failing_snapshot.store.lookup(&target).unwrap().is_none());

    canon_snapshot.store.close().unwrap();
    failing_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_keeps_non_canon_subtree_unchanged_when_recovery_fails() {
    let config = make_run_config(false, 1);
    let diagnostics = RecordingDiagnosticSink::default();
    let progress = NullSink;
    let base_executor = kitchensync::operations::executor(&config, &diagnostics, &progress);
    let executor = FailingRecoveryExecutor {
        delegate: &base_executor,
        fail_recover_directories: vec!["shared".to_string()],
        fail_peer_ids: Some(vec![120_002]),
    };
    let scheduler = make_scheduler();

    let canon_root = next_test_root("recovery_non_canon_canon");
    let non_canon_root = next_test_root("recovery_non_canon_failed");

    let canon = make_peer_session(120_001, &canon_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "recovery_non_canon_canon_db");
    let non_canon = make_peer_session(
        120_002,
        &non_canon_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut non_canon_snapshot = prepare_snapshot(&non_canon, "recovery_non_canon_failed_db");

    write_file(&canon_root, "shared/frozen.txt", "canon source");
    write_file(&non_canon_root, "shared/frozen.txt", "failing peer");

    write_file(&canon_root, "active.txt", "winner content from canon");
    write_file(&non_canon_root, "active.txt", "small");

    let frozen = kitchensync::RelPath::new("shared/frozen.txt").unwrap();
    let baseline_meta = kitchensync::EntryMeta {
        name: "frozen.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::snapshot::fresh_timestamp(),
        byte_size: 14,
    };
    non_canon_snapshot
        .store
        .upsert_confirmed_present(&frozen, &baseline_meta)
        .unwrap();
    let before_row = non_canon_snapshot
        .store
        .lookup(&frozen)
        .unwrap()
        .expect("non-canon baseline snapshot row");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &canon,
            snapshot: &mut canon_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &non_canon,
            snapshot: &mut non_canon_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &diagnostics,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(diagnostics.has_error());
    assert!(report.failures.iter().any(|failure| {
        matches!(
            failure,
            kitchensync::sync::SyncFailure::SwapRecovery {
                peer_id: 120_002,
                directory,
                canon: false,
                ..
            } if directory.as_str() == "shared"
        )
    }));

    assert_eq!(
        read_file(&non_canon_root, "shared/frozen.txt"),
        "failing peer"
    );
    let after_row = non_canon_snapshot
        .store
        .lookup(&frozen)
        .unwrap()
        .expect("non-canon snapshot row after run");
    assert_eq!(before_row, after_row);
    assert_eq!(
        read_file(&non_canon_root, "active.txt"),
        "winner content from canon"
    );

    canon_snapshot.store.close().unwrap();
    non_canon_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_keeps_all_peer_rows_and_files_unchanged_when_canon_recovery_fails() {
    let config = make_run_config(false, 1);
    let diagnostics = RecordingDiagnosticSink::default();
    let progress = NullSink;
    let base_executor = kitchensync::operations::executor(&config, &diagnostics, &progress);
    let executor = FailingRecoveryExecutor {
        delegate: &base_executor,
        fail_recover_directories: vec!["shared".to_string()],
        fail_peer_ids: Some(vec![130_001]),
    };
    let scheduler = make_scheduler();

    let canon_root = next_test_root("recovery_canon_canon");
    let other_root = next_test_root("recovery_canon_other");

    let canon = make_peer_session(130_001, &canon_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "recovery_canon_canon_db");
    let other = make_peer_session(
        130_002,
        &other_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut other_snapshot = prepare_snapshot(&other, "recovery_canon_other_db");

    write_file(&canon_root, "shared/frozen.txt", "canon baseline");
    write_file(&other_root, "shared/frozen.txt", "other baseline");

    let shared = kitchensync::RelPath::new("shared/frozen.txt").unwrap();
    let canon_meta = kitchensync::EntryMeta {
        name: "frozen.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::snapshot::fresh_timestamp(),
        byte_size: 13,
    };
    let other_meta = kitchensync::EntryMeta {
        name: "frozen.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::snapshot::fresh_timestamp(),
        byte_size: 13,
    };
    canon_snapshot
        .store
        .upsert_confirmed_present(&shared, &canon_meta)
        .unwrap();
    other_snapshot
        .store
        .upsert_confirmed_present(&shared, &other_meta)
        .unwrap();

    let before_canon = canon_snapshot
        .store
        .lookup(&shared)
        .unwrap()
        .expect("canon snapshot baseline");
    let before_other = other_snapshot
        .store
        .lookup(&shared)
        .unwrap()
        .expect("other snapshot baseline");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &canon,
            snapshot: &mut canon_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &other,
            snapshot: &mut other_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &executor,
        copy_scheduler: &scheduler,
        diagnostics: &diagnostics,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(diagnostics.has_error());
    assert!(report.failures.iter().any(|failure| {
        matches!(
            failure,
            kitchensync::sync::SyncFailure::SwapRecovery {
                peer_id: 130_001,
                directory,
                canon: true,
                ..
            } if directory.as_str() == "shared"
        )
    }));
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "shared"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::CanonListingUnavailable {
                    peer_id: 130_001
                }
            )
    }));

    assert_eq!(
        read_file(&canon_root, "shared/frozen.txt"),
        "canon baseline"
    );
    assert_eq!(
        read_file(&other_root, "shared/frozen.txt"),
        "other baseline"
    );

    let after_canon = canon_snapshot
        .store
        .lookup(&shared)
        .unwrap()
        .expect("canon snapshot after");
    let after_other = other_snapshot
        .store
        .lookup(&shared)
        .unwrap()
        .expect("other snapshot after");
    assert_eq!(before_canon, after_canon);
    assert_eq!(before_other, after_other);

    canon_snapshot.store.close().unwrap();
    other_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_records_no_contributing_skip_for_subordinate_only_directory() {
    let config = make_run_config(false, 1);
    let diagnostics = RecordingDiagnosticSink::default();
    let progress = NullSink;
    let base_executor = kitchensync::operations::executor(&config, &diagnostics, &progress);
    let executor = FailingRecoveryExecutor {
        delegate: &base_executor,
        fail_recover_directories: vec!["shared".to_string()],
        fail_peer_ids: Some(vec![140_001]),
    };
    let scheduler = make_scheduler();

    let contributor_root = next_test_root("no_contrib_contributor");
    let subordinate_root = next_test_root("no_contrib_subordinate");

    let contributor = make_peer_session(
        140_001,
        &contributor_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut contributor_snapshot = prepare_snapshot(&contributor, "no_contrib_contributor_db");
    let subordinate = make_peer_session(
        140_002,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot = prepare_snapshot(&subordinate, "no_contrib_subordinate_db");

    fs::create_dir_all(contributor_root.join("shared")).unwrap();
    write_file(
        &subordinate_root,
        "shared/only-subordinate.txt",
        "do-not-touch",
    );

    let subordinate_path = kitchensync::RelPath::new("shared/only-subordinate.txt").unwrap();
    let baseline_meta = kitchensync::EntryMeta {
        name: "only-subordinate.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::snapshot::fresh_timestamp(),
        byte_size: 11,
    };
    subordinate_snapshot
        .store
        .upsert_confirmed_present(&subordinate_path, &baseline_meta)
        .unwrap();
    let before_subordinate = subordinate_snapshot
        .store
        .lookup(&subordinate_path)
        .unwrap()
        .expect("subordinate snapshot baseline");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &contributor,
            snapshot: &mut contributor_snapshot.store,
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
        diagnostics: &diagnostics,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert!(diagnostics.has_error());
    assert!(report.failures.iter().any(|failure| {
        matches!(
            failure,
            kitchensync::sync::SyncFailure::SwapRecovery {
                peer_id: 140_001,
                directory,
                canon: false,
                ..
            } if directory.as_str() == "shared"
        )
    }));
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "shared"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed
            )
    }));

    assert_eq!(
        read_file(&subordinate_root, "shared/only-subordinate.txt"),
        "do-not-touch"
    );
    assert!(!contributor_root
        .join("shared/only-subordinate.txt")
        .exists());

    let after_subordinate = subordinate_snapshot
        .store
        .lookup(&subordinate_path)
        .unwrap()
        .expect("subordinate snapshot after");
    assert_eq!(before_subordinate, after_subordinate);

    contributor_snapshot.store.close().unwrap();
    subordinate_snapshot.store.close().unwrap();
}
