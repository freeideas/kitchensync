use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
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
    path.push(format!(
        "kitchensync-traversal-boundaries-1-{normalized}-{seq}"
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

#[derive(Default)]
struct ListTransportCounters {
    root: AtomicUsize,
    sibling: AtomicUsize,
    fail: AtomicUsize,
}

impl ListTransportCounters {
    fn root_calls(&self) -> usize {
        self.root.load(Ordering::SeqCst)
    }

    fn sibling_calls(&self) -> usize {
        self.sibling.load(Ordering::SeqCst)
    }

    fn fail_calls(&self) -> usize {
        self.fail.load(Ordering::SeqCst)
    }
}

struct ListingFailureTransport {
    inner: kitchensync::TransportHandle,
    fail_path: String,
    sibling_path: String,
    calls: Arc<ListTransportCounters>,
    fail_error: kitchensync::TransportError,
}

impl ListingFailureTransport {
    fn new(
        inner: kitchensync::TransportHandle,
        fail_path: impl Into<String>,
        sibling_path: impl Into<String>,
        calls: Arc<ListTransportCounters>,
    ) -> Self {
        Self {
            inner,
            fail_path: fail_path.into(),
            sibling_path: sibling_path.into(),
            calls,
            fail_error: kitchensync::TransportError::IoError,
        }
    }
}

impl kitchensync::TransportBackend for ListingFailureTransport {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        match path.as_str() {
            "" => {
                self.calls.root.fetch_add(1, Ordering::SeqCst);
            }
            p if p == self.fail_path => {
                self.calls.fail.fetch_add(1, Ordering::SeqCst);
                return Err(self.fail_error.clone());
            }
            p if p == self.sibling_path => {
                self.calls.sibling.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
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

#[derive(Default)]
struct OperationCalls {
    recovery: Mutex<Vec<String>>,
    cleanup: Mutex<Vec<String>>,
    displace: AtomicUsize,
    create: AtomicUsize,
    copy: AtomicUsize,
}

impl OperationCalls {
    fn record_recovery(&self, directory: &kitchensync::RelPath) {
        self.recovery
            .lock()
            .expect("recovery call log lock poisoned")
            .push(directory.as_str().to_string());
    }

    fn record_cleanup(&self, directory: &kitchensync::RelPath) {
        self.cleanup
            .lock()
            .expect("cleanup call log lock poisoned")
            .push(directory.as_str().to_string());
    }

    fn cleanup_calls(&self) -> Vec<String> {
        self.cleanup
            .lock()
            .expect("cleanup call log lock poisoned")
            .clone()
    }

    fn displace_calls(&self) -> usize {
        self.displace.load(Ordering::SeqCst)
    }

    fn create_calls(&self) -> usize {
        self.create.load(Ordering::SeqCst)
    }

    fn copy_calls(&self) -> usize {
        self.copy.load(Ordering::SeqCst)
    }
}

struct FailingRecoveryTracingExecutor<'a> {
    delegate: &'a dyn kitchensync::operations::OperationExecutor,
    calls: Arc<OperationCalls>,
    fail_recover_directories: Vec<String>,
    fail_peer_ids: Option<Vec<kitchensync::PeerId>>,
}

impl<'a> kitchensync::operations::OperationExecutor for FailingRecoveryTracingExecutor<'a> {
    fn recover_directory_swaps(
        &self,
        peer: &kitchensync::PeerSession,
        directory: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::RecoveryReport> {
        self.calls.record_recovery(directory);

        let peer_matches = match &self.fail_peer_ids {
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
        self.calls.displace.fetch_add(1, Ordering::SeqCst);
        self.delegate.displace_to_bak(peer, path, timestamp)
    }

    fn create_directory(
        &self,
        peer: &kitchensync::PeerSession,
        path: &kitchensync::RelPath,
    ) -> kitchensync::operations::OperationResult<kitchensync::operations::DirectoryCreationReport>
    {
        self.calls.create.fetch_add(1, Ordering::SeqCst);
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
        self.calls.record_cleanup(directory);
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
        self.calls.copy.fetch_add(1, Ordering::SeqCst);
        self.delegate.execute_copy_attempt(
            source_peer,
            source_path,
            destination_peer,
            destination_path,
            winning_meta,
        )
    }
}

#[test]
fn sync_run_retries_non_canon_listing_failures_for_one_peer_without_losing_sibling_traversal() {
    let config = make_run_config(false, 3);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("boundaries1-listing-retry-source");
    let destination_root = next_test_root("boundaries1-listing-retry-dest");

    let calls = Arc::new(ListTransportCounters::default());
    let listing_transport = ListingFailureTransport::new(
        kitchensync::transport::factory()
            .connect(
                &make_peer_url(&destination_root),
                kitchensync::TransportTimeouts {
                    timeout_conn: 1,
                    timeout_idle: 1,
                },
                kitchensync::TransportRootMode::CreateMissing,
            )
            .unwrap(),
        "target",
        "active",
        calls.clone(),
    );

    let source = make_peer_session(
        11_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let destination = make_peer_session_with_transport(
        11_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
        listing_transport,
    );

    create_directory(&source_root, "target");
    create_directory(&source_root, "active");
    create_directory(&destination_root, "target");
    create_directory(&destination_root, "active");

    let mut source_snapshot = prepare_snapshot(&source, "boundaries1-listing-retry-source-db");
    let mut destination_snapshot =
        prepare_snapshot(&destination, "boundaries1-listing-retry-dest-db");

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
        operations: &base_executor,
        copy_scheduler: &scheduler,
        diagnostics: &NullSink,
        progress: &NullSink,
    });

    let listing = report
        .failures
        .iter()
        .find_map(|failure| match failure {
            kitchensync::sync::SyncFailure::Listing {
                peer_id,
                directory,
                attempts,
                canon,
                ..
            } if *peer_id == 11_002 && directory.as_str() == "target" => Some((*attempts, *canon)),
            _ => None,
        })
        .expect("expected listing failure for destination peer at target");

    assert_eq!(listing.0, 3);
    assert!(!listing.1);
    assert_eq!(calls.root_calls(), 1);
    assert_eq!(calls.sibling_calls(), 1);
    assert_eq!(calls.fail_calls(), 3);
    assert_eq!(report.traversal.scanned_directories, 3);
    assert_eq!(report.traversal.decided_entries, 2);
    assert!(!report.completed);
    assert!(report.skipped.is_empty());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_keeps_cleanup_outside_a_canon_failed_listing_subtree() {
    let config = make_run_config(false, 1);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let calls = Arc::new(OperationCalls::default());
    let executor = FailingRecoveryTracingExecutor {
        delegate: &base_executor,
        calls: calls.clone(),
        fail_recover_directories: vec!["skipme".to_string()],
        fail_peer_ids: Some(vec![12_001]),
    };
    let scheduler = make_scheduler();

    let source_root = next_test_root("boundaries1-cleanup-skip-source");
    let source = make_peer_session(12_001, &source_root, kitchensync::EffectivePeerRole::Canon);
    create_directory(&source_root, "keepalive");
    create_directory(&source_root, "skipme");
    write_file(&source_root, "skipme/preserved.txt", "source");

    let mut source_snapshot = prepare_snapshot(&source, "boundaries1-cleanup-skip-source-db");

    let mut peers = [kitchensync::sync::SyncPeer {
        session: &source,
        snapshot: &mut source_snapshot.store,
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
        .any(|failure| matches!(failure, kitchensync::sync::SyncFailure::SwapRecovery { peer_id, directory, attempts: 1, canon: true, ..}
            if *peer_id == 12_001 && directory.as_str() == "skipme")));
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "skipme"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::CanonListingUnavailable { peer_id }
                    if peer_id == 12_001
            )
    }));

    let cleanup = calls.cleanup_calls();
    assert!(cleanup.iter().any(|entry| entry == ""));
    assert!(cleanup.iter().any(|entry| entry == "keepalive"));
    assert!(!cleanup.iter().any(|entry| entry == "skipme"));
    assert_eq!(
        fs::read_to_string(source_root.join("skipme/preserved.txt")).unwrap(),
        "source"
    );
    assert_eq!(calls.displace_calls(), 0);
    assert_eq!(calls.create_calls(), 0);
    assert_eq!(calls.copy_calls(), 0);

    source_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_records_no_contributing_subtree_skip_and_ignores_subordinate_only_entries() {
    let config = make_run_config(false, 2);
    let base_executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let contributing_root = next_test_root("boundaries1-no-contrib-source");
    let subordinate_root = next_test_root("boundaries1-no-contrib-subordinate");

    let calls = Arc::new(ListTransportCounters::default());
    let failing_transport = ListingFailureTransport::new(
        kitchensync::transport::factory()
            .connect(
                &make_peer_url(&contributing_root),
                kitchensync::TransportTimeouts {
                    timeout_conn: 1,
                    timeout_idle: 1,
                },
                kitchensync::TransportRootMode::CreateMissing,
            )
            .unwrap(),
        "orphans",
        "active",
        calls.clone(),
    );

    let contributor = make_peer_session_with_transport(
        13_001,
        &contributing_root,
        kitchensync::EffectivePeerRole::Contributing,
        failing_transport,
    );
    let mut contributor_snapshot =
        prepare_snapshot(&contributor, "boundaries1-no-contrib-source-db");

    let subordinate = make_peer_session(
        13_002,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot =
        prepare_snapshot(&subordinate, "boundaries1-no-contrib-subordinate-db");

    create_directory(&contributing_root, "active");
    create_directory(&contributing_root, "orphans");
    create_directory(&subordinate_root, "active");
    create_directory(&subordinate_root, "orphans");

    write_file(
        &subordinate_root,
        "orphans/only-subordinate.txt",
        "sub-only",
    );

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
    assert!(report.skipped.iter().any(|entry| {
        entry.directory.as_str() == "orphans"
            && matches!(
                entry.reason,
                kitchensync::sync::SkippedSubtreeReason::NoContributingPeerListed
            )
    }));
    let listing = report
        .failures
        .iter()
        .find_map(|failure| match failure {
            kitchensync::sync::SyncFailure::Listing {
                peer_id,
                directory,
                attempts,
                ..
            } if *peer_id == 13_001 && directory.as_str() == "orphans" => Some(*attempts),
            _ => None,
        })
        .expect("expected listing failure for contributing peer at orphans");
    assert_eq!(listing, 2);
    assert_eq!(calls.sibling_calls(), 1);
    assert_eq!(calls.fail_calls(), 2);
    assert_eq!(
        fs::read_to_string(subordinate_root.join("orphans/only-subordinate.txt")).unwrap(),
        "sub-only"
    );
    assert!(!contributing_root
        .join("orphans/only-subordinate.txt")
        .exists());

    contributor_snapshot.store.close().unwrap();
    subordinate_snapshot.store.close().unwrap();
}
