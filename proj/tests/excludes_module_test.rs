use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};

#[derive(Default)]
struct NullSink;

impl kitchensync::runtime::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::DiagnosticSink for NullSink {
    fn publish(&self, _event: kitchensync::DiagnosticEvent) {}
}

impl kitchensync::runtime::ProgressSink for NullSink {
    fn publish(&self, _event: kitchensync::ProgressEvent) {}
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
        "kitchensync-excludes-module-test-{normalized}-{seq}"
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

fn make_run_config(dry_run: bool) -> kitchensync::RunConfig {
    kitchensync::RunConfig {
        dry_run,
        max_copies: 2,
        retries_copy: 2,
        retries_list: 1,
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

#[derive(Default)]
struct TransportCallLog {
    list_dir: AtomicUsize,
    stat: AtomicUsize,
    open_read: AtomicUsize,
    open_write: AtomicUsize,
    rename_no_overwrite: AtomicUsize,
    delete_file: AtomicUsize,
    create_dir: AtomicUsize,
    delete_dir: AtomicUsize,
    set_mod_time: AtomicUsize,
}

impl TransportCallLog {
    fn reset(&self) {
        self.list_dir.store(0, Ordering::SeqCst);
        self.stat.store(0, Ordering::SeqCst);
        self.open_read.store(0, Ordering::SeqCst);
        self.open_write.store(0, Ordering::SeqCst);
        self.rename_no_overwrite.store(0, Ordering::SeqCst);
        self.delete_file.store(0, Ordering::SeqCst);
        self.create_dir.store(0, Ordering::SeqCst);
        self.delete_dir.store(0, Ordering::SeqCst);
        self.set_mod_time.store(0, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct UnsupportedMetadataBackend {
    calls: Arc<TransportCallLog>,
    candidate: kitchensync::EntryMeta,
}

impl UnsupportedMetadataBackend {
    fn with_shared_log(
        name: &str,
        kind: kitchensync::EntryKind,
        calls: Arc<TransportCallLog>,
    ) -> Self {
        Self {
            calls,
            candidate: kitchensync::EntryMeta {
                name: name.to_string(),
                kind,
                mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
                byte_size: -1,
            },
        }
    }
}

impl kitchensync::TransportBackend for UnsupportedMetadataBackend {
    fn list_dir(
        &self,
        path: &kitchensync::RelPath,
    ) -> Result<Vec<kitchensync::EntryMeta>, kitchensync::TransportError> {
        self.calls.list_dir.fetch_add(1, Ordering::SeqCst);
        if path.as_str().is_empty() {
            return Ok(vec![self.candidate.clone()]);
        }
        Err(kitchensync::TransportError::NotFound)
    }

    fn stat(
        &self,
        _path: &kitchensync::RelPath,
    ) -> Result<kitchensync::EntryMeta, kitchensync::TransportError> {
        self.calls.stat.fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }

    fn open_read(
        &self,
        _path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportRead, kitchensync::TransportError> {
        self.calls.open_read.fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }

    fn open_write(
        &self,
        _path: &kitchensync::RelPath,
    ) -> Result<kitchensync::TransportWrite, kitchensync::TransportError> {
        self.calls.open_write.fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }

    fn rename_no_overwrite(
        &self,
        _src: &kitchensync::RelPath,
        _dst: &kitchensync::RelPath,
    ) -> Result<(), kitchensync::TransportError> {
        self.calls
            .rename_no_overwrite
            .fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }

    fn delete_file(&self, _path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.calls.delete_file.fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }

    fn create_dir(&self, _path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.calls.create_dir.fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }

    fn delete_dir(&self, _path: &kitchensync::RelPath) -> Result<(), kitchensync::TransportError> {
        self.calls.delete_dir.fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }

    fn set_mod_time(
        &self,
        _path: &kitchensync::RelPath,
        _time: kitchensync::Timestamp,
    ) -> Result<(), kitchensync::TransportError> {
        self.calls.set_mod_time.fetch_add(1, Ordering::SeqCst);
        Err(kitchensync::TransportError::NotFound)
    }
}

#[test]
fn sync_run_excludes_configured_directory_subtree_before_snapshot_lookup() {
    let mut config = make_run_config(false);
    config.excludes = vec![kitchensync::RelPath::new("cache").unwrap()];

    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("command_anchor_directory");
    let destination_root = next_test_root("command_anchor_directory_dest");

    let source = make_peer_session(
        10_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "command_anchor_directory_source_snapshot");
    let destination = make_peer_session(
        10_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(
        &destination,
        "command_anchor_directory_destination_snapshot",
    );

    write_file(&source_root, "cache/legacy.txt", "do-not-touch");
    write_file(&source_root, "keep.txt", "keep-me");

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

    let excluded = kitchensync::RelPath::new("cache").unwrap();
    let excluded_child = kitchensync::RelPath::new("cache/legacy.txt").unwrap();
    let kept = kitchensync::RelPath::new("keep.txt").unwrap();

    assert!(report.completed);
    assert_eq!(report.traversal.decided_entries, 1);
    assert_eq!(report.copies.succeeded, 1);
    assert!(destination_root.join("keep.txt").exists());
    assert_eq!(
        fs::read_to_string(destination_root.join("keep.txt")).unwrap(),
        "keep-me"
    );
    assert!(!destination_root.join("cache").exists());

    assert!(source_snapshot.store.lookup(&kept).unwrap().is_some());
    assert!(destination_snapshot.store.lookup(&kept).unwrap().is_some());
    assert!(source_snapshot.store.lookup(&excluded).unwrap().is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&excluded_child)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_child)
        .unwrap()
        .is_none());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_excludes_built_in_metadata_directories_anywhere() {
    let mut config = make_run_config(false);
    config.excludes = vec![];

    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("built_in_directories_source");
    let destination_root = next_test_root("built_in_directories_destination");

    let source = make_peer_session(
        11_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "built_in_directories_source_snapshot");
    let destination = make_peer_session(
        11_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "built_in_directories_destination_snapshot");

    write_file(&source_root, "project/.git/config", "source-git");
    write_file(
        &source_root,
        "project/.kitchensync/state.bin",
        "source-state",
    );
    write_file(&source_root, "project/kept.txt", "shared");

    write_file(&destination_root, "project/.git/config", "destination-git");

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

    let kept = kitchensync::RelPath::new("project/kept.txt").unwrap();
    let excluded_git = kitchensync::RelPath::new("project/.git").unwrap();
    let excluded_git_file = kitchensync::RelPath::new("project/.git/config").unwrap();
    let excluded_state = kitchensync::RelPath::new("project/.kitchensync/state.bin").unwrap();

    assert!(report.completed);
    assert_eq!(report.traversal.scanned_directories, 2);
    assert_eq!(report.copies.succeeded, 1);
    assert!(destination_root.join("project/kept.txt").exists());
    assert!(destination_root.join("project/.git").exists());
    assert_eq!(
        fs::read_to_string(destination_root.join("project/.git/config")).unwrap(),
        "destination-git"
    );
    assert!(!destination_root
        .join("project/.kitchensync/state.bin")
        .exists());

    assert!(source_snapshot.store.lookup(&kept).unwrap().is_some());
    assert!(destination_snapshot.store.lookup(&kept).unwrap().is_some());
    assert!(source_snapshot
        .store
        .lookup(&excluded_git_file)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_git_file)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&excluded_state)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_state)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&excluded_git)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_git)
        .unwrap()
        .is_none());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_configured_excludes_do_not_disable_built_in_directory_exclusions() {
    let mut config = make_run_config(false);
    config.excludes = vec![kitchensync::RelPath::new("cache").unwrap()];

    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("built_in_excludes_with_configured_anchor_source");
    let destination_root = next_test_root("built_in_excludes_with_configured_anchor_destination");

    let source = make_peer_session(
        11_101,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(
        &source,
        "built_in_excludes_with_configured_anchor_source_snapshot",
    );
    let destination = make_peer_session(
        11_102,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(
        &destination,
        "built_in_excludes_with_configured_anchor_destination_snapshot",
    );

    write_file(&source_root, "cache/legacy.txt", "do-not-touch");
    write_file(&source_root, "project/.git/config", "source-git");
    write_file(
        &source_root,
        "project/.kitchensync/state.bin",
        "source-state",
    );
    write_file(&source_root, "project/kept.txt", "shared");

    write_file(&destination_root, "project/.git/config", "destination-git");

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

    let kept = kitchensync::RelPath::new("project/kept.txt").unwrap();
    let excluded_cache = kitchensync::RelPath::new("cache/legacy.txt").unwrap();
    let excluded_git = kitchensync::RelPath::new("project/.git/config").unwrap();
    let excluded_state = kitchensync::RelPath::new("project/.kitchensync/state.bin").unwrap();

    assert!(report.completed);
    assert_eq!(report.copies.succeeded, 1);
    assert!(destination_root.join("project/kept.txt").exists());
    assert_eq!(
        fs::read_to_string(destination_root.join("project/kept.txt")).unwrap(),
        "shared"
    );
    assert!(destination_root.join("project/.git").exists());
    assert_eq!(
        fs::read_to_string(destination_root.join("project/.git/config")).unwrap(),
        "destination-git"
    );
    assert!(!destination_root.join("cache").exists());
    assert!(!destination_root.join("project/.kitchensync").exists());

    assert!(source_snapshot.store.lookup(&kept).unwrap().is_some());
    assert!(destination_snapshot.store.lookup(&kept).unwrap().is_some());
    assert!(source_snapshot
        .store
        .lookup(&excluded_cache)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_cache)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&excluded_git)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_git)
        .unwrap()
        .is_none());
    assert!(source_snapshot
        .store
        .lookup(&excluded_state)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded_state)
        .unwrap()
        .is_none());

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}

#[test]
fn sync_run_excludes_non_regular_transport_entries_with_no_downstream_effects() {
    let config = make_run_config(false);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("unsupported_source");
    let destination_root = next_test_root("unsupported_destination");
    let shared_log = Arc::new(TransportCallLog::default());

    let source_backend = UnsupportedMetadataBackend::with_shared_log(
        "mystery.bin",
        kitchensync::EntryKind::SymbolicLink,
        Arc::clone(&shared_log),
    );
    let destination_backend = UnsupportedMetadataBackend::with_shared_log(
        "mystery.bin",
        kitchensync::EntryKind::SymbolicLink,
        Arc::clone(&shared_log),
    );

    let source = make_peer_session_with_transport(
        12_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
        source_backend,
    );
    let mut source_snapshot = prepare_snapshot(&source, "unsupported_source_snapshot");

    let destination = make_peer_session_with_transport(
        12_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
        destination_backend,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "unsupported_destination_snapshot");

    shared_log.reset();

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

    let excluded = kitchensync::RelPath::new("mystery.bin").unwrap();

    assert!(report.completed);
    assert_eq!(report.traversal.scanned_directories, 1);
    assert_eq!(report.traversal.decided_entries, 0);
    assert_eq!(report.copies.succeeded, 0);

    assert!(source_snapshot.store.lookup(&excluded).unwrap().is_none());
    assert!(destination_snapshot
        .store
        .lookup(&excluded)
        .unwrap()
        .is_none());

    assert_eq!(shared_log.open_read.load(Ordering::SeqCst), 0);
    assert_eq!(shared_log.open_write.load(Ordering::SeqCst), 0);
    assert_eq!(shared_log.rename_no_overwrite.load(Ordering::SeqCst), 0);
    assert_eq!(shared_log.delete_file.load(Ordering::SeqCst), 0);
    assert_eq!(shared_log.create_dir.load(Ordering::SeqCst), 0);
    assert_eq!(shared_log.set_mod_time.load(Ordering::SeqCst), 0);

    source_snapshot.store.close().unwrap();
    destination_snapshot.store.close().unwrap();
}
