use std::fs;
use std::path::{Path, PathBuf};
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

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    let normalized = test_name.replace(['\\', '/'], "_");
    path.push(format!("kitchensync-resp3-test-{normalized}-{seq}"));

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

#[test]
fn traversal_3_builds_candidates_from_live_listings_only() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("live_candidates_source");
    let destination_root = next_test_root("live_candidates_destination");

    let source = make_peer_session(
        30_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "live_candidates_source_db");
    let destination = make_peer_session(
        30_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot = prepare_snapshot(&destination, "live_candidates_destination_db");

    let live_file = kitchensync::RelPath::new("kept.txt").unwrap();
    let snapshot_only_file = kitchensync::RelPath::new("ghost-from-snapshot-only.log").unwrap();

    write_file(&source_root, live_file.as_str(), "from source");

    let phantom_meta = kitchensync::EntryMeta {
        name: "ghost-from-snapshot-only.log".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::snapshot::fresh_timestamp(),
        byte_size: 7,
    };

    source_snapshot
        .store
        .upsert_confirmed_present(&snapshot_only_file, &phantom_meta)
        .unwrap();
    destination_snapshot
        .store
        .upsert_confirmed_present(&snapshot_only_file, &phantom_meta)
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

    let source_ghost_row = source_snapshot
        .store
        .lookup(&snapshot_only_file)
        .unwrap()
        .expect("snapshot-only phantom path should exist");
    let destination_ghost_row = destination_snapshot
        .store
        .lookup(&snapshot_only_file)
        .unwrap()
        .expect("snapshot-only phantom path should exist");

    assert!(source_ghost_row.deleted_time.is_none());
    assert!(destination_ghost_row.deleted_time.is_none());
}

#[test]
fn traversal_3_includes_subordinate_live_names_while_contributor_remains_active() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let contributing_root = next_test_root("subordinate_inclusion_contributing");
    let subordinate_root = next_test_root("subordinate_inclusion_subordinate");

    let contributing = make_peer_session(
        31_001,
        &contributing_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut contributing_snapshot =
        prepare_snapshot(&contributing, "subordinate_inclusion_contributing_db");
    let subordinate = make_peer_session(
        31_002,
        &subordinate_root,
        kitchensync::EffectivePeerRole::Subordinate,
    );
    let mut subordinate_snapshot =
        prepare_snapshot(&subordinate, "subordinate_inclusion_subordinate_db");

    write_file(&contributing_root, "shared.txt", "source-win");
    write_file(
        &subordinate_root,
        "sub-only.txt",
        "only-subordinate-content",
    );

    let mut peers = [
        kitchensync::sync::SyncPeer {
            session: &contributing,
            snapshot: &mut contributing_snapshot.store,
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
    assert_eq!(report.traversal.decided_entries, 2);

    assert!(!subordinate_root.join("sub-only.txt").exists());
    assert!(contributing_root.join("shared.txt").exists());
    assert!(subordinate_root.join("shared.txt").exists());
}

#[test]
fn traversal_3_enforces_command_excludes_as_exact_file_and_directory_prefix() {
    let config = kitchensync::RunConfig {
        excludes: vec![
            kitchensync::RelPath::new("skip.txt").unwrap(),
            kitchensync::RelPath::new("cache").unwrap(),
        ],
        ..make_run_config(false, 1)
    };

    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("command_excludes_source");
    let destination_root = next_test_root("command_excludes_destination");

    let source = make_peer_session(
        32_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "command_excludes_source_db");
    let destination = make_peer_session(
        32_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "command_excludes_destination_db");

    write_file(&source_root, "skip.txt", "from source");
    write_file(&source_root, "cache/nested/inner.txt", "from source cache");
    write_file(&source_root, "keep.txt", "kept from source");

    write_file(
        &destination_root,
        "skip.txt",
        "preexisting source-protected",
    );
    write_file(
        &destination_root,
        "cache/nested/inner.txt",
        "preexisting cache content",
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
    assert_eq!(report.traversal.decided_entries, 1);

    assert_eq!(
        read_file(&destination_root, "skip.txt"),
        "preexisting source-protected"
    );
    assert_eq!(
        read_file(&destination_root, "cache/nested/inner.txt"),
        "preexisting cache content"
    );
    assert_eq!(read_file(&destination_root, "keep.txt"), "kept from source");

    let snapshot_keep = kitchensync::RelPath::new("keep.txt").unwrap();
    let snapshot_skip = kitchensync::RelPath::new("skip.txt").unwrap();
    let snapshot_cache_file = kitchensync::RelPath::new("cache/nested/inner.txt").unwrap();

    assert!(destination_snapshot
        .store
        .lookup(&snapshot_keep)
        .unwrap()
        .is_some());
    assert!(destination_snapshot
        .store
        .lookup(&snapshot_skip)
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&snapshot_cache_file)
        .unwrap()
        .is_none());
}

#[test]
fn traversal_3_enforces_built_in_directory_excludes() {
    let config = make_run_config(false, 1);
    let executor = kitchensync::operations::executor(&config, &NullSink, &NullSink);
    let scheduler = make_scheduler();

    let source_root = next_test_root("builtin_excludes_source");
    let destination_root = next_test_root("builtin_excludes_destination");

    let source = make_peer_session(
        33_001,
        &source_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut source_snapshot = prepare_snapshot(&source, "builtin_excludes_source_db");
    let destination = make_peer_session(
        33_002,
        &destination_root,
        kitchensync::EffectivePeerRole::Contributing,
    );
    let mut destination_snapshot =
        prepare_snapshot(&destination, "builtin_excludes_destination_db");

    write_file(&source_root, ".git/secret.txt", "from source");
    write_file(&source_root, ".kitchensync/notes.txt", "from source");
    write_file(&source_root, "normal.txt", "from source");

    write_file(&destination_root, ".git/secret.txt", "kept secret");
    write_file(
        &destination_root,
        ".kitchensync/notes.txt",
        "kept metadata note",
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
    assert_eq!(report.traversal.decided_entries, 1);

    assert_eq!(read_file(&destination_root, "normal.txt"), "from source");
    assert_eq!(
        read_file(&destination_root, ".git/secret.txt"),
        "kept secret"
    );
    assert_eq!(
        read_file(&destination_root, ".kitchensync/notes.txt"),
        "kept metadata note"
    );

    assert!(destination_snapshot
        .store
        .lookup(&kitchensync::RelPath::new("normal.txt").unwrap())
        .unwrap()
        .is_some());
    assert!(destination_snapshot
        .store
        .lookup(&kitchensync::RelPath::new(".git/secret.txt").unwrap())
        .unwrap()
        .is_none());
    assert!(destination_snapshot
        .store
        .lookup(&kitchensync::RelPath::new(".kitchensync/notes.txt").unwrap())
        .unwrap()
        .is_none());
}
