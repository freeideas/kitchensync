use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "kitchensync_snapshot_{}_{}",
        test_name.replace(['\\', '/'], "_"),
        seq
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

fn make_peer_session(id: kitchensync::PeerId, root: &Path) -> kitchensync::PeerSession {
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

    kitchensync::PeerSession {
        id,
        invocation_index: 0,
        normalized_identity,
        selected_url,
        declared_role: kitchensync::PeerRole::Normal,
        effective_role: kitchensync::EffectivePeerRole::Contributing,
        transport,
        had_startup_snapshot: false,
    }
}

fn create_invalid_snapshot_database(root: &Path) {
    let db_path = root.join(".kitchensync/snapshot.db");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch("CREATE TABLE snapshot (id INTEGER PRIMARY KEY);")
        .unwrap();
}

struct ListedPaths {
    listed: HashSet<String>,
}

impl ListedPaths {
    fn new(paths: &[&str]) -> Self {
        Self {
            listed: paths.iter().map(|path| (*path).to_string()).collect(),
        }
    }
}

impl kitchensync::snapshot::SnapshotListedPaths for ListedPaths {
    fn contains(&self, path: &kitchensync::RelPath) -> bool {
        self.listed.contains(path.as_str())
    }
}

#[test]
fn snapshot_fresh_timestamp_is_strictly_increasing() {
    let first = kitchensync::snapshot::fresh_timestamp();
    let second = kitchensync::snapshot::fresh_timestamp();
    let third = kitchensync::snapshot::fresh_timestamp();

    assert!(first.0 < second.0);
    assert!(second.0 < third.0);
}

#[test]
fn prepare_peer_snapshot_missing_live_snapshot_reports_no_history() {
    let root = next_test_root("prepare_missing_live_snapshot");
    let peer = make_peer_session(1, &root);
    let tmp_root = next_test_root("prepare_missing_live_snapshot_tmp");

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(!opened.had_history_at_startup);
    assert_eq!(opened.store.peer(), peer.id);
    assert!(!opened.store.had_changes());

    let missing = kitchensync::RelPath::new("missing/path.txt").unwrap();
    assert!(opened.store.lookup(&missing).unwrap().is_none());

    opened.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_rejects_invalid_database_schema() {
    let root = next_test_root("prepare_invalid_snapshot_schema");
    create_invalid_snapshot_database(&root);
    let peer = make_peer_session(2, &root);
    let tmp_root = next_test_root("prepare_invalid_snapshot_schema_tmp");

    let result = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    );

    match result {
        Err(kitchensync::snapshot::SnapshotError::InvalidDatabase {
            reason: kitchensync::snapshot::SnapshotDatabaseError::SchemaMismatch,
            ..
        }) => {}
        other => panic!("expected schema mismatch error, got {other:?}"),
    }
}

#[test]
fn prepare_peer_snapshot_normal_mode_recover_stale_swap_old_and_new_files() {
    let root = next_test_root("prepare_recover_swap_old_new");
    let peer = make_peer_session(3, &root);
    let seed_tmp = next_test_root("prepare_recover_swap_old_new_seed");

    let path = kitchensync::RelPath::new("docs/readme.txt").unwrap();
    let meta = kitchensync::EntryMeta {
        name: "readme.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        byte_size: 13,
    };

    {
        let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
            &peer,
            &seed_tmp,
            kitchensync::snapshot::SnapshotStartupMode::Normal,
        )
        .unwrap();
        opened.store.upsert_confirmed_present(&path, &meta).unwrap();
        let closed = opened.store.close().unwrap();
        kitchensync::snapshot::upload_peer_snapshot(&peer, closed).unwrap();
    }

    let live = root.join(".kitchensync/snapshot.db");
    let swap_root = root.join(".kitchensync/SWAP/snapshot.db");
    let swap_old = swap_root.join("old");
    let swap_new = swap_root.join("new");

    fs::create_dir_all(&swap_root).unwrap();
    fs::copy(&live, &swap_new).unwrap();
    fs::copy(&live, &swap_old).unwrap();

    let verify_tmp = next_test_root("prepare_recover_swap_old_new_verify");
    let verified = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &verify_tmp,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(verified.had_history_at_startup);
    assert!(!swap_new.exists(), "normal startup must remove stale swap/new");
    assert!(!swap_old.exists(), "normal startup must remove stale swap/old");
    assert!(verified.store.lookup(&path).unwrap().is_some());

    verified.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_dry_run_skips_snapshot_swap_recovery() {
    let root = next_test_root("prepare_dry_run_skips_recovery");
    let peer = make_peer_session(4, &root);

    let swap_new = root.join(".kitchensync/SWAP/snapshot.db/new");
    fs::create_dir_all(swap_new.parent().unwrap()).unwrap();
    fs::write(&swap_new, b"stale").unwrap();

    let tmp_root = next_test_root("prepare_dry_run_skip_recovery_tmp");
    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::DryRun,
    )
    .unwrap();

    assert!(!opened.had_history_at_startup);
    assert!(swap_new.exists(), "dry-run must not recover stale swap/new");
    assert!(!root.join(".kitchensync/snapshot.db").exists());

    opened.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_dry_run_downloads_live_snapshot_without_recovery() {
    let root = next_test_root("dry_run_keeps_swap_new_without_recovery");
    let peer = make_peer_session(5, &root);
    let seed_tmp = next_test_root("dry_run_keeps_swap_new_without_recovery_seed");

    let seed_path = kitchensync::RelPath::new("seed.txt").unwrap();
    let meta = kitchensync::EntryMeta {
        name: "seed.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-01_00-00-00_000000Z".to_string()),
        byte_size: 7,
    };

    {
        let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
            &peer,
            &seed_tmp,
            kitchensync::snapshot::SnapshotStartupMode::Normal,
        )
        .unwrap();
        opened.store.upsert_confirmed_present(&seed_path, &meta).unwrap();
        let closed = opened.store.close().unwrap();
        kitchensync::snapshot::upload_peer_snapshot(&peer, closed).unwrap();
    }

    let swap_new = root.join(".kitchensync/SWAP/snapshot.db/new");
    fs::create_dir_all(swap_new.parent().unwrap()).unwrap();
    fs::write(&swap_new, b"stale").unwrap();

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("prepare_dry_run_downloads_live_snapshot_tmp"),
        kitchensync::snapshot::SnapshotStartupMode::DryRun,
    )
    .unwrap();

    assert!(opened.had_history_at_startup);
    assert!(swap_new.exists(), "dry-run must not touch swap/new");
    assert!(opened
        .store
        .lookup(&seed_path)
        .unwrap()
        .is_some());

    opened.store.close().unwrap();
}

#[test]
fn snapshot_lookup_reports_entry_kind_from_byte_size() {
    let root = next_test_root("snapshot_lookup_reports_kind");
    let peer = make_peer_session(6, &root);
    let tmp_root = next_test_root("snapshot_lookup_reports_kind_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let dir_path = kitchensync::RelPath::new("docs").unwrap();
    let file_path = kitchensync::RelPath::new("docs/notes.txt").unwrap();

    let dir_meta = kitchensync::EntryMeta {
        name: "docs".to_string(),
        kind: kitchensync::EntryKind::Directory,
        mod_time: kitchensync::Timestamp("2024-01-10_00-00-00_000000Z".to_string()),
        byte_size: -1,
    };
    let file_meta = kitchensync::EntryMeta {
        name: "notes.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-10_00-01-00_000000Z".to_string()),
        byte_size: 21,
    };

    opened.store.upsert_confirmed_present(&dir_path, &dir_meta).unwrap();
    opened.store.upsert_confirmed_present(&file_path, &file_meta).unwrap();

    let dir_row = opened.store.lookup(&dir_path).unwrap().unwrap();
    let file_row = opened.store.lookup(&file_path).unwrap().unwrap();

    assert!(matches!(dir_row.kind, kitchensync::snapshot::SnapshotEntryKind::Directory));
    assert!(matches!(file_row.kind, kitchensync::snapshot::SnapshotEntryKind::File));
    assert_eq!(dir_row.byte_size, -1);
    assert_eq!(file_row.byte_size, 21);

    opened.store.close().unwrap();
}

#[test]
fn snapshot_mutations_update_expected_timestamps_and_sizes() {
    let root = next_test_root("mutation_updates_timestamps");
    let peer = make_peer_session(7, &root);
    let tmp_root = next_test_root("mutation_updates_timestamps_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("assets/data.bin").unwrap();
    let confirmed = kitchensync::EntryMeta {
        name: "data.bin".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-01_12-00-00_000000Z".to_string()),
        byte_size: 256,
    };
    let first_seen = opened
        .store
        .upsert_confirmed_present(&path, &confirmed)
        .unwrap();

    let after_confirmed = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after_confirmed.last_seen, Some(first_seen.clone()));
    assert_eq!(after_confirmed.deleted_time, None);
    assert_eq!(after_confirmed.mod_time, confirmed.mod_time);
    assert_eq!(after_confirmed.byte_size, 256);

    let intended = kitchensync::EntryMeta {
        name: "data.bin".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-02_12-00-00_000000Z".to_string()),
        byte_size: 512,
    };
    opened.store.upsert_intended_copy(&path, &intended).unwrap();

    let after_intended = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after_intended.last_seen, Some(first_seen.clone()));
    assert_eq!(after_intended.mod_time, intended.mod_time);
    assert_eq!(after_intended.byte_size, 512);

    let complete_seen = opened.store.mark_copy_complete(&path).unwrap();
    let after_complete = opened.store.lookup(&path).unwrap().unwrap();
    assert!(complete_seen.0 > first_seen.0);
    assert_eq!(after_complete.last_seen, Some(complete_seen));

    assert!(opened.store.had_changes());
    opened.store.close().unwrap();
}

#[test]
fn upsert_intended_copy_first_time_keeps_last_seen_null() {
    let root = next_test_root("intended_copy_first_time");
    let peer = make_peer_session(8, &root);
    let tmp_root = next_test_root("intended_copy_first_time_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("todo/new.txt").unwrap();
    let intended = kitchensync::EntryMeta {
        name: "new.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-03_08-30-00_000000Z".to_string()),
        byte_size: 42,
    };

    opened.store.upsert_intended_copy(&path, &intended).unwrap();
    let row = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, None);
    assert_eq!(row.deleted_time, None);
    assert_eq!(row.byte_size, 42);

    opened.store.close().unwrap();
}

#[test]
fn upsert_intended_copy_clears_tombstone_and_preserves_last_seen() {
    let root = next_test_root("intended_copy_clears_tombstone");
    let peer = make_peer_session(9, &root);
    let tmp_root = next_test_root("intended_copy_clears_tombstone_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("cache/index.dat").unwrap();
    let present = kitchensync::EntryMeta {
        name: "index.dat".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-04_09-00-00_000000Z".to_string()),
        byte_size: 9,
    };

    let last_seen = opened.store.upsert_confirmed_present(&path, &present).unwrap();
    opened.store.mark_absent(&path).unwrap();

    let intended = kitchensync::EntryMeta {
        name: "index.dat".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-04_10-00-00_000000Z".to_string()),
        byte_size: 10,
    };

    opened.store.upsert_intended_copy(&path, &intended).unwrap();

    let row = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, Some(last_seen));
    assert_eq!(row.deleted_time, None);
    assert_eq!(row.mod_time, intended.mod_time);

    opened.store.close().unwrap();
}

#[test]
fn mark_absent_is_idempotent_and_preserves_last_seen() {
    let root = next_test_root("mark_absent_idempotent");
    let peer = make_peer_session(10, &root);
    let tmp_root = next_test_root("mark_absent_idempotent_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("cache/index.dat").unwrap();
    let present = kitchensync::EntryMeta {
        name: "index.dat".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-05_09-00-00_000000Z".to_string()),
        byte_size: 9,
    };

    let last_seen = opened.store.upsert_confirmed_present(&path, &present).unwrap();
    opened.store.mark_absent(&path).unwrap();

    let after_first = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after_first.last_seen, Some(last_seen.clone()));
    assert_eq!(after_first.deleted_time, Some(last_seen.clone()));

    opened.store.mark_absent(&path).unwrap();
    let after_second = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(after_second.last_seen, Some(last_seen));

    opened.store.close().unwrap();
}

#[test]
fn mark_displaced_directory_cascades_to_active_descendants_only() {
    let root = next_test_root("mark_displaced_directory");
    let peer = make_peer_session(11, &root);
    let tmp_root = next_test_root("mark_displaced_directory_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let root_dir = kitchensync::RelPath::new("docs").unwrap();
    let active_child = kitchensync::RelPath::new("docs/readme.txt").unwrap();
    let skipped_dir = kitchensync::RelPath::new("docs/skip").unwrap();
    let skipped_child = kitchensync::RelPath::new("docs/skip/old.txt").unwrap();
    let archive_dir = kitchensync::RelPath::new("docs/archive").unwrap();
    let archive_child = kitchensync::RelPath::new("docs/archive/history.txt").unwrap();

    let dir_meta = kitchensync::EntryMeta {
        name: "docs".to_string(),
        kind: kitchensync::EntryKind::Directory,
        mod_time: kitchensync::Timestamp("2024-01-06_00-00-00_000000Z".to_string()),
        byte_size: -1,
    };
    let file_meta = kitchensync::EntryMeta {
        name: "readme.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-06_00-01-00_000000Z".to_string()),
        byte_size: 100,
    };
    let skipped_dir_meta = kitchensync::EntryMeta {
        name: "skip".to_string(),
        kind: kitchensync::EntryKind::Directory,
        mod_time: kitchensync::Timestamp("2024-01-06_00-02-00_000000Z".to_string()),
        byte_size: -1,
    };
    let skip_child_meta = kitchensync::EntryMeta {
        name: "old.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-06_00-03-00_000000Z".to_string()),
        byte_size: 11,
    };
    let archive_meta = kitchensync::EntryMeta {
        name: "archive".to_string(),
        kind: kitchensync::EntryKind::Directory,
        mod_time: kitchensync::Timestamp("2024-01-06_00-04-00_000000Z".to_string()),
        byte_size: -1,
    };
    let archive_child_meta = kitchensync::EntryMeta {
        name: "history.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-06_00-05-00_000000Z".to_string()),
        byte_size: 22,
    };

    opened
        .store
        .upsert_confirmed_present(&root_dir, &dir_meta)
        .unwrap();
    let expected_deletion_estimate = opened.store.lookup(&root_dir).unwrap().unwrap().last_seen.unwrap();

    opened
        .store
        .upsert_confirmed_present(&active_child, &file_meta)
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(&skipped_dir, &skipped_dir_meta)
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(&skipped_child, &skip_child_meta)
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(&archive_dir, &archive_meta)
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(&archive_child, &archive_child_meta)
        .unwrap();

    opened.store.mark_absent(&skipped_dir).unwrap();

    opened
        .store
        .mark_displaced(&root_dir, kitchensync::snapshot::SnapshotEntryKind::Directory)
        .unwrap();

    let root_after = opened.store.lookup(&root_dir).unwrap().unwrap();
    let active_after = opened.store.lookup(&active_child).unwrap().unwrap();
    let archive_after = opened.store.lookup(&archive_dir).unwrap().unwrap();
    let archive_child_after = opened.store.lookup(&archive_child).unwrap().unwrap();
    let skipped_dir_after = opened.store.lookup(&skipped_dir).unwrap().unwrap();
    let skipped_child_after = opened.store.lookup(&skipped_child).unwrap().unwrap();

    assert_eq!(root_after.deleted_time, Some(expected_deletion_estimate));
    assert_eq!(active_after.deleted_time, Some(root_after.deleted_time.clone().unwrap()));
    assert_eq!(archive_after.deleted_time, Some(root_after.deleted_time.clone().unwrap()));
    assert_eq!(archive_child_after.deleted_time, root_after.deleted_time);
    assert!(skipped_dir_after.deleted_time.is_some());
    assert!(skipped_child_after.deleted_time.is_none());

    opened.store.close().unwrap();
}

#[test]
fn cleanup_stale_rows_removes_unlisted_row_and_keeps_listed_path() {
    let root = next_test_root("cleanup_retention");
    let peer = make_peer_session(12, &root);
    let tmp_root = next_test_root("cleanup_retention_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let keep = kitchensync::RelPath::new("keep.txt").unwrap();
    let remove = kitchensync::RelPath::new("remove.txt").unwrap();

    let keep_meta = kitchensync::EntryMeta {
        name: "keep.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-07_00-00-00_000000Z".to_string()),
        byte_size: 3,
    };
    let remove_meta = kitchensync::EntryMeta {
        name: "remove.txt".to_string(),
        kind: kitchensync::EntryKind::File,
        mod_time: kitchensync::Timestamp("2024-01-07_00-00-01_000000Z".to_string()),
        byte_size: 3,
    };

    opened.store.upsert_confirmed_present(&keep, &keep_meta).unwrap();
    opened.store.upsert_confirmed_present(&remove, &remove_meta).unwrap();

    let listed_paths = ListedPaths::new(&["keep.txt"]);
    opened
        .store
        .cleanup_stale_rows(kitchensync::snapshot::SnapshotCleanupScope {
            listed_paths: &listed_paths,
            retention: kitchensync::RetentionPolicy {
                keep_tmp_days: 0,
                keep_bak_days: 0,
                keep_del_days: 0,
            },
        })
        .unwrap();

    assert!(opened.store.lookup(&keep).unwrap().is_some());
    assert!(opened.store.lookup(&remove).unwrap().is_none());

    opened.store.close().unwrap();
}
