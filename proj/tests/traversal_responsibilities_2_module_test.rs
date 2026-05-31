use std::collections::HashMap;
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

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let normalized = test_name.replace(['\\', '/'], "_");
    let mut path = std::env::temp_dir();
    path.push(format!("kitchensync-traversal-resp-2-{normalized}-{seq}"));

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

#[derive(Clone)]
struct FlakyListingTransport {
    inner: kitchensync::TransportHandle,
    fail_attempt_limit: Arc<HashMap<String, usize>>,
    list_attempts: Arc<Mutex<HashMap<String, usize>>>,
}

impl FlakyListingTransport {
    fn new(inner: kitchensync::TransportHandle, failures: Vec<(String, usize)>) -> Self {
        Self {
            inner,
            fail_attempt_limit: Arc::new(failures.into_iter().collect()),
            list_attempts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn record_list_dir_attempt(&self, path: &str) -> usize {
        let mut attempts = self
            .list_attempts
            .lock()
            .expect("list attempt lock poisoned");
        let count = attempts.entry(path.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    fn list_attempts_for(&self, path: &str) -> usize {
        self.list_attempts
            .lock()
            .expect("list attempt lock poisoned")
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    fn should_fail_listing(&self, path: &str, attempt: usize) -> bool {
        self.fail_attempt_limit
            .get(path)
            .is_some_and(|max_failures| attempt <= *max_failures)
    }
}

impl kitchensync::TransportBackend for FlakyListingTransport {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        let current_attempt = self.record_list_dir_attempt(path.as_str());
        if self.should_fail_listing(path.as_str(), current_attempt) {
            return Err(kitchensync::TransportError::IoError);
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
struct FailingRecoveryExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    fail_rules: Vec<(kitchensync::PeerId, String)>,
    recovery_attempts: Arc<Mutex<HashMap<kitchensync::PeerId, HashMap<String, usize>>>>,
}

impl<'a> FailingRecoveryExecutor<'a> {
    fn new(
        delegate: &'a dyn kitchensync::operations::OperationExecutor,
        fail_rules: Vec<(kitchensync::PeerId, String)>,
    ) -> Self {
        Self {
            delegate,
            fail_rules,
            recovery_attempts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn recovery_attempts_for(&self, peer_id: kitchensync::PeerId, directory: &str) -> usize {
        self.recovery_attempts
            .lock()
            .expect("recovery attempt lock poisoned")
            .get(&peer_id)
            .and_then(|for_peer| for_peer.get(directory))
            .copied()
            .unwrap_or(0)
    }

    fn should_fail(&self, peer_id: kitchensync::PeerId, directory: &str) -> bool {
        self.fail_rules.iter().any(|(target_id, target_directory)| {
            *target_id == peer_id && target_directory == directory
        })
    }

    fn record_recovery_attempt(&self, peer_id: kitchensync::PeerId, directory: &str) {
        let mut attempts = self
            .recovery_attempts
            .lock()
            .expect("recovery attempt lock poisoned");
        let peer_attempts = attempts.entry(peer_id).or_insert_with(HashMap::new);
        *peer_attempts.entry(directory.to_string()).or_insert(0) += 1;
    }
}

impl<'a> kitchensync::operations::OperationExecutor for FailingRecoveryExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        self.record_recovery_attempt(peer.id, directory.as_str());

        if self.should_fail(peer.id, directory.as_str()) {
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

fn listing_failure_attempts(
    report: &kitchensync::sync::SyncReport,
    peer_id: kitchensync::PeerId,
    directory: &str,
) -> Option<usize> {
    report.failures.iter().find_map(|failure| match failure {
        kitchensync::sync::SyncFailure::Listing {
            peer_id: reported_peer_id,
            directory: reported_directory,
            attempts,
            ..
        } if *reported_peer_id == peer_id && reported_directory.as_str() == directory => {
            Some(*attempts)
        }
        _ => None,
    })
}

fn swap_recovery_failure_attempts(
    report: &kitchensync::sync::SyncReport,
    peer_id: kitchensync::PeerId,
    directory: &str,
) -> Option<usize> {
    report.failures.iter().find_map(|failure| match failure {
        kitchensync::sync::SyncFailure::SwapRecovery {
            peer_id: reported_peer_id,
            directory: reported_directory,
            attempts,
            ..
        } if *reported_peer_id == peer_id && reported_directory.as_str() == directory => {
            Some(*attempts)
        }
        _ => None,
    })
}

#[test]
fn sync_run_retries_failed_directory_listing_until_success() {
    let config = make_run_config(false, 3);
    let scheduler = make_scheduler();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    let source_root = next_test_root("listing_retry_root_success_source");
    let mut source_snapshot = prepare_snapshot(
        &make_peer_session(
            201_001,
            &source_root,
            kitchensync::EffectivePeerRole::Contributing,
        ),
        "listing_retry_root_success_source_db",
    );

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
    let source_transport =
        FlakyListingTransport::new(source_transport, vec![(String::from(""), 2)]);
    let source = make_peer_session_with_transport(
        201_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        source_transport.clone(),
    );

    write_file(&source_root, "keep.txt", "kept");

    let mut peers = [kitchensync::sync::SyncPeer {
        session: &source,
        snapshot: &mut source_snapshot.store,
    }];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &base_executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);
    assert_eq!(source_transport.list_attempts_for(""), 3);
    assert!(listing_failure_attempts(&report, 201_001, "").is_none());

    source_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_scopes_non_canon_listing_failure_to_the_directory() {
    let config = make_run_config(false, 2);
    let scheduler = make_scheduler();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    let contributor_root = next_test_root("listing_scoping_contributor_root");
    let fallback_root = next_test_root("listing_scoping_fallback_root");

    write_file(&contributor_root, "blocked/kept.txt", "source");
    write_file(&contributor_root, "shared/propagated.txt", "winner");
    fs::create_dir_all(fallback_root.join("blocked")).unwrap();
    fs::create_dir_all(fallback_root.join("shared")).unwrap();
    write_file(&fallback_root, "blocked/old.txt", "fallback-old");

    let contributor = make_peer_session(
        202_001,
        &contributor_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut contributor_snapshot = prepare_snapshot(&contributor, "listing_scoping_contributor_db");

    let failing_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&fallback_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();
    let failing_transport =
        FlakyListingTransport::new(failing_transport, vec![(String::from("blocked"), 2)]);
    let failing = make_peer_session_with_transport(
        202_002,
        &fallback_root,
        kitchensync::EffectivePeerRole::Contributing,
        failing_transport.clone(),
    );
    let mut fallback_snapshot = prepare_snapshot(&failing, "listing_scoping_fallback_db");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &contributor,
            snapshot: &mut contributor_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &failing,
            snapshot: &mut fallback_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &base_executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert_eq!(
        listing_failure_attempts(&report, 202_002, "blocked"),
        Some(2)
    );
    assert_eq!(failing_transport.list_attempts_for("blocked"), 2);
    assert_eq!(read_file(&fallback_root, "shared/propagated.txt"), "winner");
    assert_eq!(read_file(&fallback_root, "blocked/old.txt"), "fallback-old");

    contributor_snapshot.store.close().unwrap();
    fallback_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_scopes_failed_pre_listing_recovery_to_the_directory() {
    let config = make_run_config(false, 3);
    let scheduler = make_scheduler();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let recovery_guard =
        FailingRecoveryExecutor::new(&base_executor, vec![(302_002, String::from("blocked"))]);

    let contributor_root = next_test_root("recovery_scoping_contributor_root");
    let fallback_root = next_test_root("recovery_scoping_fallback_root");

    write_file(&contributor_root, "blocked/kept.txt", "source");
    write_file(&contributor_root, "shared/propagated.txt", "winner");
    fs::create_dir_all(fallback_root.join("blocked")).unwrap();
    fs::create_dir_all(fallback_root.join("shared")).unwrap();
    write_file(&fallback_root, "blocked/old.txt", "fallback-old");

    let contributor = make_peer_session(
        302_001,
        &contributor_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut contributor_snapshot =
        prepare_snapshot(&contributor, "recovery_scoping_contributor_db");

    let fallback = make_peer_session(
        302_002,
        &fallback_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut fallback_snapshot = prepare_snapshot(&fallback, "recovery_scoping_fallback_db");

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &contributor,
            snapshot: &mut contributor_snapshot.store,
        },
        kitchensync::sync::SyncPeer {
            session: &fallback,
            snapshot: &mut fallback_snapshot.store,
        },
    ];

    let report = kitchensync::sync::run(kitchensync::sync::SyncRun {
        config: &config,
        peers: &mut peers,
        operations: &recovery_guard,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert_eq!(
        swap_recovery_failure_attempts(&report, 302_002, "blocked"),
        Some(3)
    );
    assert_eq!(recovery_guard.recovery_attempts_for(302_002, "blocked"), 3);
    assert_eq!(read_file(&fallback_root, "shared/propagated.txt"), "winner");
    assert_eq!(read_file(&fallback_root, "blocked/old.txt"), "fallback-old");

    contributor_snapshot.store.close().unwrap();
    fallback_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_skips_subtree_when_no_contributing_peer_remains() {
    let config = make_run_config(false, 2);
    let scheduler = make_scheduler();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);

    let contributor_root = next_test_root("no_contributing_skip_contributor_root");
    let subordinate_root = next_test_root("no_contributing_skip_subordinate_root");

    write_file(&contributor_root, "root.txt", "root");
    write_file(&contributor_root, "shared/none.txt", "missing-contributor");
    write_file(&subordinate_root, "shared/subordinate-only.txt", "stay");
    fs::create_dir_all(contributor_root.join("shared")).unwrap();
    fs::create_dir_all(subordinate_root.join("shared")).unwrap();

    let contributor_transport = kitchensync::transport::factory()
        .connect(
            &make_peer_url(&contributor_root),
            kitchensync::TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            kitchensync::TransportRootMode::CreateMissing,
        )
        .unwrap();
    let contributor_transport =
        FlakyListingTransport::new(contributor_transport, vec![(String::from("shared"), 2)]);
    let contributor = make_peer_session_with_transport(
        303_001,
        &contributor_root,
        kitchensync::EffectivePeerRole::Contributing,
        contributor_transport,
    );
    let mut contributor_snapshot =
        prepare_snapshot(&contributor, "no_contributing_skip_contributor_db");

    let subordinate = make_peer_session(
        303_002,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot =
        prepare_snapshot(&subordinate, "no_contributing_skip_subordinate_db");

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
        operations: &base_executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert_eq!(
        listing_failure_attempts(&report, 303_001, "shared"),
        Some(2)
    );
    assert!(report
        .skipped
        .iter()
        .any(|entry| entry.directory.as_str() == "shared"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed
            )));
    assert_eq!(
        read_file(&subordinate_root, "shared/subordinate-only.txt"),
        "stay"
    );

    contributor_snapshot.store.close().unwrap();
    subordinate_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_skips_entire_subtree_when_canon_fails_pre_listing_recovery() {
    let config = make_run_config(false, 2);
    let scheduler = make_scheduler();
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let recovery_guard =
        FailingRecoveryExecutor::new(&base_executor, vec![(304_001, String::from("blocked"))]);

    let canon_root = next_test_root("canon_fail_prelisting_canon_root");
    let contributor_root = next_test_root("canon_fail_prelisting_contributor_root");

    write_file(&canon_root, "carry.txt", "carry");
    fs::create_dir_all(canon_root.join("blocked")).unwrap();
    write_file(&canon_root, "blocked/keep.txt", "canon");
    write_file(&contributor_root, "blocked/stay.txt", "stay");

    let canon = make_peer_session(304_001, &canon_root, kitchensync::EffectivePeerRole::Canon);
    let mut canon_snapshot = prepare_snapshot(&canon, "canon_fail_prelisting_canon_db");

    let contributor = make_peer_session(
        304_002,
        &contributor_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut contributor_snapshot =
        prepare_snapshot(&contributor, "canon_fail_prelisting_contributor_db");

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
        operations: &recovery_guard,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    assert!(!report.completed);
    assert_eq!(
        swap_recovery_failure_attempts(&report, 304_001, "blocked"),
        Some(2)
    );
    assert_eq!(recovery_guard.recovery_attempts_for(304_001, "blocked"), 2);
    assert!(report
        .skipped
        .iter()
        .any(|entry| entry.directory.as_str() == "blocked" && matches!(entry.reason, kitchensync::sync::SkippedSubtreeReason::CanonListingUnavailable { peer_id } if peer_id == 304_001)));
    assert_eq!(read_file(&canon_root, "carry.txt"), "carry");
    assert_eq!(read_file(&contributor_root, "blocked/stay.txt"), "stay");
    assert_eq!(read_file(&canon_root, "blocked/keep.txt"), "canon");

    canon_snapshot.store.close().unwrap();
    contributor_snapshot.store.close().unwrap();
}
