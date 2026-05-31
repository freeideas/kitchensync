use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let sequence = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "kitchensync_snapshot_module_{test_name}_{sequence}",
        test_name = test_name.replace(['\\', '/'], "_"),
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

fn make_meta(
    name: &str,
    kind: kitchensync::EntryKind,
    mod_time: &str,
    byte_size: i64,
) -> kitchensync::EntryMeta {
    kitchensync::EntryMeta {
        name: name.to_string(),
        kind,
        mod_time: kitchensync::Timestamp(mod_time.to_string()),
        byte_size,
    }
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
fn fresh_timestamp_is_strictly_increasing() {
    let first = kitchensync::snapshot::fresh_timestamp();
    let second = kitchensync::snapshot::fresh_timestamp();
    let third = kitchensync::snapshot::fresh_timestamp();

    assert!(first.0 < second.0);
    assert!(second.0 < third.0);
}

#[test]
fn prepare_peer_snapshot_missing_live_snapshot_reports_no_history() {
    let root = next_test_root("prepare_missing_history");
    let peer = make_peer_session(100, &root);
    let tmp_root = next_test_root("prepare_missing_history_tmp");

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(!opened.had_history_at_startup);
    assert!(!opened.store.had_changes());
    assert!(opened
        .store
        .lookup(&kitchensync::RelPath::new("x.txt").unwrap())
        .unwrap()
        .is_none());

    opened.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_rejects_invalid_schema() {
    let root = next_test_root("prepare_invalid_schema");
    create_invalid_snapshot_database(&root);
    let peer = make_peer_session(101, &root);
    let tmp_root = next_test_root("prepare_invalid_schema_tmp");

    let result = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    );

    assert!(matches!(
        result,
        Err(kitchensync::snapshot::SnapshotError::InvalidDatabase {
            reason: kitchensync::snapshot::SnapshotDatabaseError::SchemaMismatch,
            ..
        })
    ));
}

#[test]
fn prepare_peer_snapshot_normal_mode_deletes_stale_swap_when_live_exists() {
    let root = next_test_root("prepare_recover_old_new_live");
    let peer = make_peer_session(102, &root);
    let seed_tmp = next_test_root("prepare_recover_old_new_live_seed");

    let path = kitchensync::RelPath::new("docs/readme.txt").unwrap();
    let meta = make_meta(
        "readme.txt",
        kitchensync::EntryKind::File,
        "2024-01-01_00-00-00_000000Z",
        64,
    );

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

    fs::create_dir_all(swap_root.parent().unwrap()).unwrap();
    fs::copy(&live, &swap_old).unwrap();
    fs::copy(&live, &swap_new).unwrap();

    let verify_tmp = next_test_root("prepare_recover_old_new_live_verify");
    let verified = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &verify_tmp,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(verified.had_history_at_startup);
    assert!(!swap_old.exists());
    assert!(!swap_new.exists());
    assert!(verified.store.lookup(&path).unwrap().is_some());

    verified.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_normal_mode_recovers_new_only_when_live_missing() {
    let root = next_test_root("prepare_recover_new_only");
    let peer = make_peer_session(103, &root);
    let tmp_seed = next_test_root("prepare_recover_new_only_seed");
    let path = kitchensync::RelPath::new("seed.txt").unwrap();
    let meta = make_meta(
        "seed.txt",
        kitchensync::EntryKind::File,
        "2024-01-01_00-00-00_000000Z",
        7,
    );

    {
        let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
            &peer,
            &tmp_seed,
            kitchensync::snapshot::SnapshotStartupMode::Normal,
        )
        .unwrap();
        opened.store.upsert_confirmed_present(&path, &meta).unwrap();
        let closed = opened.store.close().unwrap();
        kitchensync::snapshot::upload_peer_snapshot(&peer, closed).unwrap();
    }

    let live = root.join(".kitchensync/snapshot.db");
    let backup = live.with_file_name("snapshot-backup.db");
    fs::copy(&live, &backup).unwrap();
    let swap_new = root.join(".kitchensync/SWAP/snapshot.db/new");
    fs::create_dir_all(swap_new.parent().unwrap()).unwrap();
    fs::copy(&backup, &swap_new).unwrap();
    fs::remove_file(&live).unwrap();

    let prepared = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("prepare_recover_new_only_verify"),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(prepared.had_history_at_startup);
    assert!(!swap_new.exists());
    assert!(prepared.store.lookup(&path).unwrap().is_some());

    prepared.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_dry_run_skips_swap_recovery() {
    let root = next_test_root("prepare_dry_run_skip_recovery");
    let peer = make_peer_session(104, &root);
    let seed_tmp = next_test_root("prepare_dry_run_skip_recovery_seed");
    let path = kitchensync::RelPath::new("seed.txt").unwrap();
    let meta = make_meta(
        "seed.txt",
        kitchensync::EntryKind::File,
        "2024-01-01_00-00-00_000000Z",
        10,
    );

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

    let swap_new = root.join(".kitchensync/SWAP/snapshot.db/new");
    fs::create_dir_all(swap_new.parent().unwrap()).unwrap();
    fs::write(&swap_new, b"stale").unwrap();

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("prepare_dry_run_skip_recovery_verify"),
        kitchensync::snapshot::SnapshotStartupMode::DryRun,
    )
    .unwrap();

    assert!(opened.had_history_at_startup);
    assert!(swap_new.exists());
    assert!(opened.store.lookup(&path).unwrap().is_some());

    opened.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_dry_run_without_live_snapshot_reports_no_history() {
    let root = next_test_root("prepare_dry_run_missing_history");
    let peer = make_peer_session(127, &root);
    let tmp_root = next_test_root("prepare_dry_run_missing_history_tmp");

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::DryRun,
    )
    .unwrap();

    assert!(!opened.had_history_at_startup);
    assert!(opened
        .store
        .lookup(&kitchensync::RelPath::new("x.txt").unwrap())
        .unwrap()
        .is_none());
    assert!(!opened.store.had_changes());

    opened.store.close().unwrap();
}

#[test]
fn dry_run_snapshot_changes_are_not_uploaded() {
    let root = next_test_root("prepare_dry_run_local_only");
    let peer = make_peer_session(128, &root);
    let tmp_root = next_test_root("prepare_dry_run_local_only_tmp");

    let path = kitchensync::RelPath::new("local-only.txt").unwrap();
    let meta = make_meta(
        "local-only.txt",
        kitchensync::EntryKind::File,
        "2024-03-07_00-00-00_000000Z",
        12,
    );

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::DryRun,
    )
    .unwrap();
    opened.store.upsert_confirmed_present(&path, &meta).unwrap();
    opened.store.close().unwrap();

    let reopened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("prepare_dry_run_local_only_verify"),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(!reopened.had_history_at_startup);
    assert!(reopened.store.lookup(&path).unwrap().is_none());

    reopened.store.close().unwrap();
}

#[test]
fn snapshot_lookup_reports_file_and_directory_kinds() {
    let root = next_test_root("snapshot_lookup_kind");
    let peer = make_peer_session(105, &root);
    let tmp_root = next_test_root("snapshot_lookup_kind_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let dir_path = kitchensync::RelPath::new("docs").unwrap();
    let file_path = kitchensync::RelPath::new("docs/readme.txt").unwrap();

    let dir_meta = make_meta(
        "docs",
        kitchensync::EntryKind::Directory,
        "2024-02-01_00-00-00_000000Z",
        -1,
    );
    let file_meta = make_meta(
        "readme.txt",
        kitchensync::EntryKind::File,
        "2024-02-01_00-01-00_000000Z",
        42,
    );

    opened
        .store
        .upsert_confirmed_present(&dir_path, &dir_meta)
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(&file_path, &file_meta)
        .unwrap();

    let dir_row = opened.store.lookup(&dir_path).unwrap().unwrap();
    let file_row = opened.store.lookup(&file_path).unwrap().unwrap();

    assert!(matches!(
        dir_row.kind,
        kitchensync::snapshot::SnapshotEntryKind::Directory
    ));
    assert_eq!(dir_row.byte_size, -1);
    assert!(matches!(
        file_row.kind,
        kitchensync::snapshot::SnapshotEntryKind::File
    ));
    assert_eq!(file_row.byte_size, 42);

    opened.store.close().unwrap();
}

#[test]
fn snapshot_lookup_root_relative_path_has_no_row() {
    let root = next_test_root("snapshot_lookup_root");
    let peer = make_peer_session(106, &root);
    let tmp_root = next_test_root("snapshot_lookup_root_tmp");

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let root_path = kitchensync::RelPath::new("").unwrap();
    let result = opened.store.lookup(&root_path).unwrap();

    assert!(result.is_none());

    opened.store.close().unwrap();
}

#[test]
fn snapshot_store_peer_reports_peer_id() {
    let root = next_test_root("snapshot_peer_id");
    let peer = make_peer_session(126, &root);
    let tmp_root = next_test_root("snapshot_peer_id_tmp");

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert_eq!(opened.store.peer(), peer.id);

    opened.store.close().unwrap();
}

#[test]
fn upsert_confirmed_present_records_last_seen_and_sizes() {
    let root = next_test_root("upsert_confirmed_present");
    let peer = make_peer_session(107, &root);
    let tmp_root = next_test_root("upsert_confirmed_present_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("file.bin").unwrap();
    let confirmed = make_meta(
        "file.bin",
        kitchensync::EntryKind::File,
        "2024-02-02_00-00-00_000000Z",
        128,
    );
    let seen = opened
        .store
        .upsert_confirmed_present(&path, &confirmed)
        .unwrap();

    let row = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, Some(seen.clone()));
    assert_eq!(row.deleted_time, None);
    assert_eq!(row.byte_size, 128);
    assert_eq!(row.mod_time, confirmed.mod_time);

    opened.store.close().unwrap();
}

#[test]
fn upsert_intended_copy_first_time_keeps_last_seen_null() {
    let root = next_test_root("upsert_intended_first_time");
    let peer = make_peer_session(108, &root);
    let tmp_root = next_test_root("upsert_intended_first_time_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("dest.txt").unwrap();
    let intended = make_meta(
        "dest.txt",
        kitchensync::EntryKind::File,
        "2024-03-01_00-00-00_000000Z",
        77,
    );

    opened.store.upsert_intended_copy(&path, &intended).unwrap();
    let row = opened.store.lookup(&path).unwrap().unwrap();

    assert_eq!(row.last_seen, None);
    assert_eq!(row.deleted_time, None);
    assert_eq!(row.byte_size, 77);
    assert_eq!(row.mod_time, intended.mod_time);

    opened.store.close().unwrap();
}

#[test]
fn upsert_intended_copy_preserves_existing_last_seen() {
    let root = next_test_root("upsert_intended_preserves_last_seen");
    let peer = make_peer_session(109, &root);
    let tmp_root = next_test_root("upsert_intended_preserves_last_seen_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("dest.txt").unwrap();
    let prior = make_meta(
        "dest.txt",
        kitchensync::EntryKind::File,
        "2024-03-02_00-00-00_000000Z",
        1,
    );
    let prior_seen = opened
        .store
        .upsert_confirmed_present(&path, &prior)
        .unwrap();

    let replacement = make_meta(
        "dest.txt",
        kitchensync::EntryKind::File,
        "2024-03-03_00-00-00_000000Z",
        2,
    );
    opened
        .store
        .upsert_intended_copy(&path, &replacement)
        .unwrap();

    let row = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, Some(prior_seen));
    assert_eq!(row.mod_time, replacement.mod_time);
    assert_eq!(row.byte_size, 2);

    opened.store.close().unwrap();
}

#[test]
fn mark_copy_complete_advances_last_seen() {
    let root = next_test_root("mark_copy_complete");
    let peer = make_peer_session(110, &root);
    let tmp_root = next_test_root("mark_copy_complete_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("docs/readme.txt").unwrap();
    let confirmed = make_meta(
        "readme.txt",
        kitchensync::EntryKind::File,
        "2024-03-04_00-00-00_000000Z",
        14,
    );
    let initial_seen = opened
        .store
        .upsert_confirmed_present(&path, &confirmed)
        .unwrap();

    let completed = opened.store.mark_copy_complete(&path).unwrap();
    let row = opened.store.lookup(&path).unwrap().unwrap();

    assert!(completed.0 > initial_seen.0);
    assert_eq!(row.last_seen, Some(completed));

    opened.store.close().unwrap();
}

#[test]
fn mark_absent_is_idempotent_when_row_is_tombstoned() {
    let root = next_test_root("mark_absent_idempotent");
    let peer = make_peer_session(111, &root);
    let tmp_root = next_test_root("mark_absent_idempotent_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("cache/index.dat").unwrap();
    let meta = make_meta(
        "index.dat",
        kitchensync::EntryKind::File,
        "2024-03-05_00-00-00_000000Z",
        9,
    );
    let last_seen = opened.store.upsert_confirmed_present(&path, &meta).unwrap();

    opened.store.mark_absent(&path).unwrap();
    opened.store.mark_absent(&path).unwrap();

    let row = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, Some(last_seen.clone()));
    assert_eq!(row.deleted_time, Some(last_seen));

    opened.store.close().unwrap();
}

#[test]
fn mark_absent_noop_for_missing_path() {
    let root = next_test_root("mark_absent_noop");
    let peer = make_peer_session(112, &root);
    let tmp_root = next_test_root("mark_absent_noop_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let missing = kitchensync::RelPath::new("missing.txt").unwrap();
    assert!(!opened.store.had_changes());

    opened.store.mark_absent(&missing).unwrap();
    assert!(opened.store.lookup(&missing).unwrap().is_none());
    assert!(!opened.store.had_changes());

    opened.store.close().unwrap();
}

#[test]
fn mark_displaced_file_copies_last_seen_to_deleted_time() {
    let root = next_test_root("mark_displaced_file");
    let peer = make_peer_session(113, &root);
    let tmp_root = next_test_root("mark_displaced_file_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("docs/readme.txt").unwrap();
    let meta = make_meta(
        "readme.txt",
        kitchensync::EntryKind::File,
        "2024-03-06_00-00-00_000000Z",
        99,
    );
    let last_seen = opened.store.upsert_confirmed_present(&path, &meta).unwrap();

    opened
        .store
        .mark_displaced(&path, kitchensync::EntryKind::File)
        .unwrap();

    let row = opened.store.lookup(&path).unwrap().unwrap();
    assert_eq!(row.last_seen, Some(last_seen.clone()));
    assert_eq!(row.deleted_time, Some(last_seen));

    opened.store.close().unwrap();
}

#[test]
fn mark_displaced_directory_cascades_active_descendants_only() {
    let root = next_test_root("mark_displaced_directory");
    let peer = make_peer_session(114, &root);
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

    opened
        .store
        .upsert_confirmed_present(
            &root_dir,
            &make_meta(
                "docs",
                kitchensync::EntryKind::Directory,
                "2024-01-01_00-00-00_000000Z",
                -1,
            ),
        )
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(
            &active_child,
            &make_meta(
                "readme.txt",
                kitchensync::EntryKind::File,
                "2024-01-01_00-00-01_000000Z",
                11,
            ),
        )
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(
            &skipped_dir,
            &make_meta(
                "skip",
                kitchensync::EntryKind::Directory,
                "2024-01-01_00-00-02_000000Z",
                -1,
            ),
        )
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(
            &skipped_child,
            &make_meta(
                "old.txt",
                kitchensync::EntryKind::File,
                "2024-01-01_00-00-03_000000Z",
                4,
            ),
        )
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(
            &archive_dir,
            &make_meta(
                "archive",
                kitchensync::EntryKind::Directory,
                "2024-01-01_00-00-04_000000Z",
                -1,
            ),
        )
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(
            &archive_child,
            &make_meta(
                "history.txt",
                kitchensync::EntryKind::File,
                "2024-01-01_00-00-05_000000Z",
                8,
            ),
        )
        .unwrap();
    opened.store.mark_absent(&skipped_dir).unwrap();

    let expected_deleted = opened
        .store
        .lookup(&root_dir)
        .unwrap()
        .unwrap()
        .last_seen
        .expect("displaced directory has initial last_seen");

    opened
        .store
        .mark_displaced(&root_dir, kitchensync::EntryKind::Directory)
        .unwrap();

    let root_after = opened.store.lookup(&root_dir).unwrap().unwrap();
    let active_after = opened.store.lookup(&active_child).unwrap().unwrap();
    let skipped_dir_after = opened.store.lookup(&skipped_dir).unwrap().unwrap();
    let skipped_child_after = opened.store.lookup(&skipped_child).unwrap().unwrap();
    let archive_after = opened.store.lookup(&archive_dir).unwrap().unwrap();
    let archive_child_after = opened.store.lookup(&archive_child).unwrap().unwrap();

    assert_eq!(root_after.deleted_time, Some(expected_deleted.clone()));
    assert_eq!(active_after.deleted_time, Some(expected_deleted.clone()));
    assert_eq!(archive_after.deleted_time, Some(expected_deleted.clone()));
    assert_eq!(archive_child_after.deleted_time, Some(expected_deleted));

    assert!(skipped_dir_after.deleted_time.is_some());
    assert!(skipped_child_after.deleted_time.is_none());

    opened.store.close().unwrap();
}

#[test]
fn cleanup_stale_rows_removes_obsolete_and_tombstone_rows() {
    let root = next_test_root("cleanup_stale_rows");
    let peer = make_peer_session(115, &root);
    let tmp_root = next_test_root("cleanup_stale_rows_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let listed_path = kitchensync::RelPath::new("kept.txt").unwrap();
    let tombstone_path = kitchensync::RelPath::new("obsolete-tombstone.txt").unwrap();
    let intended_path = kitchensync::RelPath::new("obsolete-intended.txt").unwrap();

    opened
        .store
        .upsert_confirmed_present(
            &listed_path,
            &make_meta(
                "kept.txt",
                kitchensync::EntryKind::File,
                "2024-04-01_00-00-00_000000Z",
                1,
            ),
        )
        .unwrap();
    opened
        .store
        .upsert_confirmed_present(
            &tombstone_path,
            &make_meta(
                "obsolete-tombstone.txt",
                kitchensync::EntryKind::File,
                "2024-04-01_00-00-01_000000Z",
                2,
            ),
        )
        .unwrap();
    opened.store.mark_absent(&tombstone_path).unwrap();
    opened
        .store
        .upsert_intended_copy(
            &intended_path,
            &make_meta(
                "obsolete-intended.txt",
                kitchensync::EntryKind::File,
                "2024-04-01_00-00-02_000000Z",
                3,
            ),
        )
        .unwrap();

    let listed = ListedPaths::new(&["kept.txt"]);
    opened
        .store
        .cleanup_stale_rows(kitchensync::snapshot::SnapshotCleanupScope {
            listed_paths: &listed,
            keep_del_days: 0,
        })
        .unwrap();

    assert!(opened.store.lookup(&listed_path).unwrap().is_some());
    assert!(opened.store.lookup(&tombstone_path).unwrap().is_none());
    assert!(opened.store.lookup(&intended_path).unwrap().is_none());

    opened.store.close().unwrap();
}

#[test]
fn snapshot_had_changes_tracks_mutation_state() {
    let root = next_test_root("snapshot_had_changes");
    let peer = make_peer_session(116, &root);
    let tmp_root = next_test_root("snapshot_had_changes_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let missing = kitchensync::RelPath::new("missing.txt").unwrap();
    assert!(!opened.store.had_changes());

    opened.store.mark_absent(&missing).unwrap();
    assert!(!opened.store.had_changes());

    let path = kitchensync::RelPath::new("present.txt").unwrap();
    opened
        .store
        .upsert_intended_copy(
            &path,
            &make_meta(
                "present.txt",
                kitchensync::EntryKind::File,
                "2024-01-01_00-00-00_000000Z",
                1,
            ),
        )
        .unwrap();
    assert!(opened.store.had_changes());

    opened.store.close().unwrap();
}

#[test]
fn upload_peer_snapshot_round_trip_updates_peer_snapshot() {
    let root = next_test_root("upload_peer_snapshot_round_trip");
    let peer = make_peer_session(117, &root);
    let tmp_root = next_test_root("upload_peer_snapshot_round_trip_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("roundtrip.txt").unwrap();
    opened
        .store
        .upsert_confirmed_present(
            &path,
            &make_meta(
                "roundtrip.txt",
                kitchensync::EntryKind::File,
                "2024-04-10_00-00-00_000000Z",
                11,
            ),
        )
        .unwrap();
    let local = opened.store.close().unwrap();
    kitchensync::snapshot::upload_peer_snapshot(&peer, local).unwrap();

    let reopened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("upload_peer_snapshot_round_trip_verify"),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(reopened.had_history_at_startup);
    assert!(reopened.store.lookup(&path).unwrap().is_some());

    reopened.store.close().unwrap();
}

#[test]
fn upload_peer_snapshot_rejects_mismatched_peer_id() {
    let peer_a_root = next_test_root("upload_mismatch_peer_a");
    let peer_a = make_peer_session(118, &peer_a_root);
    let peer_b = make_peer_session(119, &peer_a_root);
    let tmp_root = next_test_root("upload_mismatch_peer_tmp");

    let opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer_a,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let closed = opened.store.close().unwrap();
    let result = kitchensync::snapshot::upload_peer_snapshot(&peer_b, closed);

    assert!(result.is_err());
}

#[test]
fn snapshot_lookup_reports_tombstone_kind() {
    let root = next_test_root("snapshot_lookup_tombstone");
    let peer = make_peer_session(130, &root);
    let tmp_root = next_test_root("snapshot_lookup_tombstone_tmp");

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &tmp_root,
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let path = kitchensync::RelPath::new("removed.txt").unwrap();
    let meta = make_meta(
        "removed.txt",
        kitchensync::EntryKind::File,
        "2024-02-01_00-00-00_000000Z",
        3,
    );

    opened.store.upsert_confirmed_present(&path, &meta).unwrap();
    opened.store.mark_absent(&path).unwrap();

    let row = opened.store.lookup(&path).unwrap().unwrap();

    assert!(matches!(
        row.kind,
        kitchensync::snapshot::SnapshotEntryKind::Tombstone
    ));
    assert_eq!(row.byte_size, 3);
    assert!(row.deleted_time.is_some());
    assert_eq!(row.last_seen, row.deleted_time);

    opened.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_normal_mode_recovers_old_when_live_missing() {
    let root = next_test_root("prepare_recover_old_only");
    let peer = make_peer_session(125, &root);
    let seed_tmp = next_test_root("prepare_recover_old_only_seed");
    let path = kitchensync::RelPath::new("restore.txt").unwrap();
    let meta = make_meta(
        "restore.txt",
        kitchensync::EntryKind::File,
        "2024-01-01_00-00-00_000000Z",
        4,
    );

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
    fs::create_dir_all(swap_root.parent().unwrap()).unwrap();
    fs::rename(&live, &swap_old).unwrap();

    let prepared = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("prepare_recover_old_only_verify"),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(prepared.had_history_at_startup);
    assert!(!swap_old.exists());
    assert!(prepared.store.lookup(&path).unwrap().is_some());

    prepared.store.close().unwrap();
}

#[test]
fn prepare_peer_snapshot_normal_mode_deletes_new_when_old_is_missing() {
    let root = next_test_root("prepare_recover_new_and_live");
    let peer = make_peer_session(140, &root);
    let seed_tmp = next_test_root("prepare_recover_new_and_live_seed");
    let path = kitchensync::RelPath::new("seed.txt").unwrap();
    let meta = make_meta(
        "seed.txt",
        kitchensync::EntryKind::File,
        "2024-01-01_00-00-00_000000Z",
        7,
    );

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
    let swap_new = root.join(".kitchensync/SWAP/snapshot.db/new");
    fs::create_dir_all(swap_new.parent().unwrap()).unwrap();
    fs::copy(&live, &swap_new).unwrap();

    let prepared = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("prepare_recover_new_and_live_verify"),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(prepared.had_history_at_startup);
    assert!(!swap_new.exists());
    assert!(prepared.store.lookup(&path).unwrap().is_some());

    prepared.store.close().unwrap();
}

#[test]
fn flush_does_not_upload_peer_snapshot() {
    let root = next_test_root("snapshot_flush");
    let peer = make_peer_session(141, &root);
    let base_tmp = next_test_root("snapshot_flush_seed_tmp");
    let seed_path = kitchensync::RelPath::new("seed.txt").unwrap();

    {
        let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
            &peer,
            &base_tmp,
            kitchensync::snapshot::SnapshotStartupMode::Normal,
        )
        .unwrap();
        opened
            .store
            .upsert_confirmed_present(
                &seed_path,
                &make_meta(
                    "seed.txt",
                    kitchensync::EntryKind::File,
                    "2024-01-01_00-00-00_000000Z",
                    1,
                ),
            )
            .unwrap();
        let closed = opened.store.close().unwrap();
        kitchensync::snapshot::upload_peer_snapshot(&peer, closed).unwrap();
    }

    let mut opened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("snapshot_flush_tmp"),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    let local_only_path = kitchensync::RelPath::new("flush-local-only.txt").unwrap();
    opened
        .store
        .upsert_intended_copy(
            &local_only_path,
            &make_meta(
                "flush-local-only.txt",
                kitchensync::EntryKind::File,
                "2024-02-02_00-00-00_000000Z",
                3,
            ),
        )
        .unwrap();

    opened.store.flush().unwrap();
    opened.store.close().unwrap();

    let reopened = kitchensync::snapshot::prepare_peer_snapshot(
        &peer,
        &next_test_root("snapshot_flush_verify"),
        kitchensync::snapshot::SnapshotStartupMode::Normal,
    )
    .unwrap();

    assert!(reopened.store.lookup(&seed_path).unwrap().is_some());
    assert!(reopened.store.lookup(&local_only_path).unwrap().is_none());

    reopened.store.close().unwrap();
}
