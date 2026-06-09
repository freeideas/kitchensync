use snapshot_store::{Store, new};

fn tmp_db(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("snapshot_store_test_{}.db", name));
    let _ = std::fs::remove_file(&p);
    p
}

fn make_store() -> std::sync::Arc<dyn Store> {
    new(snapshot_clock::new(), snapshot_identity::new())
}

// 013.1, 013.2, 013.3: initialize creates a usable database with the snapshot schema
#[test]
fn initialize_succeeds() {
    let db = tmp_db("initialize_succeeds");
    let store = make_store();
    store.initialize(&db).expect("initialize must succeed");
}

// 013.1, 013.2, 013.3: initialize is idempotent -- calling it twice does not error
#[test]
fn initialize_is_idempotent() {
    let db = tmp_db("initialize_is_idempotent");
    let store = make_store();
    store.initialize(&db).expect("first initialize must succeed");
    store.initialize(&db).expect("second initialize must succeed");
}

// 013.4, 013.5: after initialize, read_row for an unknown path returns None
#[test]
fn read_row_unknown_path_returns_none() {
    let db = tmp_db("read_row_unknown");
    let store = make_store();
    store.initialize(&db).unwrap();
    let row = store.read_row(&db, "no/such/path.txt").unwrap();
    assert!(row.is_none(), "unknown path must return None");
}

// 013.20: writing the same path twice replaces the row -- at most one row per path
#[test]
fn at_most_one_row_per_path() {
    let db = tmp_db("at_most_one_row");
    let store = make_store();
    store.initialize(&db).unwrap();
    let path = "docs/report.pdf";
    store.record_present(&db, path, "2024-01-01_00-00-00_000000Z", 1000).unwrap();
    store.record_present(&db, path, "2024-01-02_00-00-00_000000Z", 2000).unwrap();
    let row = store.read_row(&db, path).unwrap().expect("row must exist");
    assert_eq!(row.byte_size, 2000, "second upsert must replace the first row");
}

// 013.7, 013.8: basename is the final path segment
#[test]
fn row_basename_is_final_segment() {
    let db = tmp_db("row_basename");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "docs/report.pdf", "2024-01-01_00-00-00_000000Z", 1024).unwrap();
    let row = store.read_row(&db, "docs/report.pdf").unwrap().expect("row must exist");
    assert_eq!(row.basename, "report.pdf");
}

// 013.13: a snapshot row for a file stores the file's size in byte_size
#[test]
fn file_byte_size_stored() {
    let db = tmp_db("file_byte_size");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "data/file.bin", "2024-01-01_00-00-00_000000Z", 98765).unwrap();
    let row = store.read_row(&db, "data/file.bin").unwrap().expect("row must exist");
    assert_eq!(row.byte_size, 98765);
}

// 013.14: a snapshot row for a directory stores -1 in byte_size
#[test]
fn directory_byte_size_is_negative_one() {
    let db = tmp_db("dir_byte_size");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "docs/reports", "2024-01-01_00-00-00_000000Z", -1).unwrap();
    let row = store.read_row(&db, "docs/reports").unwrap().expect("row must exist");
    assert_eq!(row.byte_size, -1);
}

// 017.1: confirmed present records the entry's current mod_time
#[test]
fn record_present_stores_mod_time() {
    let db = tmp_db("present_mod_time");
    let store = make_store();
    store.initialize(&db).unwrap();
    let mod_time = "2024-06-01_12-00-00_000000Z";
    store.record_present(&db, "file.txt", mod_time, 512).unwrap();
    let row = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    assert_eq!(row.mod_time, mod_time);
}

// 017.2: confirmed present records the entry's current byte_size
#[test]
fn record_present_stores_byte_size() {
    let db = tmp_db("present_byte_size");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "file.txt", "2024-06-01_12-00-00_000000Z", 4096).unwrap();
    let row = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    assert_eq!(row.byte_size, 4096);
}

// 017.3: confirmed present sets last_seen to a fresh timestamp
#[test]
fn record_present_sets_last_seen() {
    let db = tmp_db("present_last_seen");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "file.txt", "2024-06-01_12-00-00_000000Z", 100).unwrap();
    let row = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    assert!(row.last_seen.is_some(), "last_seen must be set after record_present");
}

// 017.4: confirmed present sets deleted_time to NULL, including when it was previously set
#[test]
fn record_present_clears_deleted_time() {
    let db = tmp_db("present_clears_deleted");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "file.txt", "2024-06-01_12-00-00_000000Z", 100).unwrap();
    store.record_absent(&db, "file.txt").unwrap();
    store.record_present(&db, "file.txt", "2024-06-02_12-00-00_000000Z", 100).unwrap();
    let row = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    assert!(row.deleted_time.is_none(), "record_present must set deleted_time to NULL");
}

// 017.5: confirmed absent on a live row copies last_seen into deleted_time
#[test]
fn record_absent_on_live_row_copies_last_seen() {
    let db = tmp_db("absent_copies_last_seen");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "file.txt", "2024-06-01_12-00-00_000000Z", 100).unwrap();
    let before = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    store.record_absent(&db, "file.txt").unwrap();
    let after = store.read_row(&db, "file.txt").unwrap().expect("row must still exist");
    assert_eq!(
        after.deleted_time, before.last_seen,
        "deleted_time must equal the row's last_seen value at the time record_absent was called"
    );
}

// 017.6: confirmed absent leaves last_seen unchanged
#[test]
fn record_absent_leaves_last_seen_unchanged() {
    let db = tmp_db("absent_last_seen_unchanged");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "file.txt", "2024-06-01_12-00-00_000000Z", 100).unwrap();
    let before = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    store.record_absent(&db, "file.txt").unwrap();
    let after = store.read_row(&db, "file.txt").unwrap().expect("row must still exist");
    assert_eq!(after.last_seen, before.last_seen, "last_seen must not change after record_absent");
}

// 017.7: confirmed absent on a row that already has deleted_time set leaves it unchanged
#[test]
fn record_absent_idempotent_on_tombstoned_row() {
    let db = tmp_db("absent_idempotent");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "file.txt", "2024-06-01_12-00-00_000000Z", 100).unwrap();
    store.record_absent(&db, "file.txt").unwrap();
    let after_first = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    store.record_absent(&db, "file.txt").unwrap();
    let after_second = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    assert_eq!(after_second.deleted_time, after_first.deleted_time, "second record_absent must not change deleted_time");
    assert_eq!(after_second.last_seen, after_first.last_seen, "second record_absent must not change last_seen");
    assert_eq!(after_second.mod_time, after_first.mod_time, "second record_absent must not change mod_time");
    assert_eq!(after_second.byte_size, after_first.byte_size, "second record_absent must not change byte_size");
}

// 017.8: push decision records the winning entry's mod_time
#[test]
fn record_push_stores_mod_time() {
    let db = tmp_db("push_mod_time");
    let store = make_store();
    store.initialize(&db).unwrap();
    let mod_time = "2024-05-15_08-30-00_000000Z";
    store.record_push(&db, "incoming.txt", mod_time, 256).unwrap();
    let row = store.read_row(&db, "incoming.txt").unwrap().expect("row must exist");
    assert_eq!(row.mod_time, mod_time);
}

// 017.9: push decision records the winning entry's byte_size
#[test]
fn record_push_stores_byte_size() {
    let db = tmp_db("push_byte_size");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_push(&db, "incoming.txt", "2024-05-15_08-30-00_000000Z", 8192).unwrap();
    let row = store.read_row(&db, "incoming.txt").unwrap().expect("row must exist");
    assert_eq!(row.byte_size, 8192);
}

// 017.10: push decision sets deleted_time to NULL
#[test]
fn record_push_clears_deleted_time() {
    let db = tmp_db("push_clears_deleted");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_push(&db, "file.txt", "2024-05-15_08-30-00_000000Z", 100).unwrap();
    let row = store.read_row(&db, "file.txt").unwrap().expect("row must exist");
    assert!(row.deleted_time.is_none(), "push decision must set deleted_time to NULL");
}

// 017.11: push decision does not set last_seen; new row has last_seen NULL
#[test]
fn record_push_does_not_set_last_seen() {
    let db = tmp_db("push_no_last_seen");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_push(&db, "new_file.txt", "2024-05-15_08-30-00_000000Z", 512).unwrap();
    let row = store.read_row(&db, "new_file.txt").unwrap().expect("row must exist");
    assert!(row.last_seen.is_none(), "push decision must leave last_seen NULL for a new row");
}

// 017.12: copy completed sets last_seen to a fresh timestamp
#[test]
fn record_copied_sets_last_seen() {
    let db = tmp_db("copied_last_seen");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_push(&db, "copied_file.txt", "2024-05-15_08-30-00_000000Z", 512).unwrap();
    store.record_copied(&db, "copied_file.txt").unwrap();
    let row = store.read_row(&db, "copied_file.txt").unwrap().expect("row must exist");
    assert!(row.last_seen.is_some(), "record_copied must set last_seen");
}

// 017.13: inline directory created sets last_seen to a fresh timestamp
#[test]
fn record_copied_sets_last_seen_for_directory() {
    let db = tmp_db("copied_dir_last_seen");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_push(&db, "new_dir", "2024-05-15_08-30-00_000000Z", -1).unwrap();
    store.record_copied(&db, "new_dir").unwrap();
    let row = store.read_row(&db, "new_dir").unwrap().expect("row must exist");
    assert!(row.last_seen.is_some(), "record_copied for a directory must set last_seen");
}

// 017.14: inline operation failed leaves the existing row unchanged
#[test]
fn record_inline_failed_leaves_row_unchanged() {
    let db = tmp_db("inline_failed");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "stable.txt", "2024-06-01_10-00-00_000000Z", 999).unwrap();
    let before = store.read_row(&db, "stable.txt").unwrap().expect("row must exist");
    store.record_inline_failed(&db, "stable.txt");
    let after = store.read_row(&db, "stable.txt").unwrap().expect("row must still exist");
    assert_eq!(after.mod_time, before.mod_time);
    assert_eq!(after.byte_size, before.byte_size);
    assert_eq!(after.last_seen, before.last_seen);
    assert_eq!(after.deleted_time, before.deleted_time);
}

// 017.21: when a run exits before a copy completes, the row keeps deleted_time NULL
#[test]
fn interrupted_copy_keeps_deleted_time_null() {
    let db = tmp_db("interrupted_copy_deleted");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_push(&db, "queued_file.txt", "2024-05-01_09-00-00_000000Z", 1024).unwrap();
    let row = store.read_row(&db, "queued_file.txt").unwrap().expect("row must exist");
    assert!(row.deleted_time.is_none(), "queued-but-never-copied row must keep deleted_time NULL");
}

// 017.22: when a run exits before a copy completes, last_seen stays NULL for a first-time target
#[test]
fn interrupted_copy_keeps_last_seen_null_for_new_target() {
    let db = tmp_db("interrupted_copy_last_seen");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_push(&db, "new_target.txt", "2024-05-01_09-00-00_000000Z", 2048).unwrap();
    let row = store.read_row(&db, "new_target.txt").unwrap().expect("row must exist");
    assert!(row.last_seen.is_none(), "queued-but-never-copied first-time target must keep last_seen NULL");
}

// 017.15: after displacement, the displaced entry's deleted_time is set to its last_seen
#[test]
fn record_displaced_sets_deleted_time_on_entry() {
    let db = tmp_db("displaced_entry");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "dir_to_displace", "2024-06-01_00-00-00_000000Z", -1).unwrap();
    let before = store.read_row(&db, "dir_to_displace").unwrap().expect("row must exist");
    let expected_deleted_time = before.last_seen.clone().expect("last_seen must be set before displacement");
    store.record_displaced(&db, "dir_to_displace").unwrap();
    let after = store.read_row(&db, "dir_to_displace").unwrap().expect("row must still exist");
    assert_eq!(
        after.deleted_time,
        Some(expected_deleted_time),
        "displaced entry's deleted_time must equal its last_seen"
    );
}

// 017.16, 017.17: displacement cascade tombstones descendants, not unrelated rows
#[test]
fn record_displaced_cascades_to_descendants_only() {
    let db = tmp_db("displaced_cascade");
    let store = make_store();
    store.initialize(&db).unwrap();
    let mod_time = "2024-06-01_00-00-00_000000Z";
    store.record_present(&db, "parent/subdir", mod_time, -1).unwrap();
    store.record_present(&db, "parent/subdir/child.txt", mod_time, 100).unwrap();
    store.record_present(&db, "unrelated/other.txt", mod_time, 50).unwrap();
    store.record_displaced(&db, "parent/subdir").unwrap();
    let child = store.read_row(&db, "parent/subdir/child.txt").unwrap().expect("child row must exist");
    let unrelated = store.read_row(&db, "unrelated/other.txt").unwrap().expect("unrelated row must exist");
    assert!(child.deleted_time.is_some(), "descendant must be tombstoned by cascade");
    assert!(unrelated.deleted_time.is_none(), "unrelated row must not be touched by cascade");
}

// 017.16: displacement cascade is transitive through parent_id links
#[test]
fn record_displaced_cascades_transitively() {
    let db = tmp_db("displaced_transitive");
    let store = make_store();
    store.initialize(&db).unwrap();
    let mod_time = "2024-06-01_00-00-00_000000Z";
    store.record_present(&db, "root/parent", mod_time, -1).unwrap();
    store.record_present(&db, "root/parent/child", mod_time, -1).unwrap();
    store.record_present(&db, "root/parent/child/grandchild.txt", mod_time, 100).unwrap();
    store.record_displaced(&db, "root/parent").unwrap();
    let child = store.read_row(&db, "root/parent/child").unwrap().expect("child row must exist");
    let grandchild = store.read_row(&db, "root/parent/child/grandchild.txt").unwrap().expect("grandchild row must exist");
    assert!(child.deleted_time.is_some(), "child must be tombstoned by cascade");
    assert!(grandchild.deleted_time.is_some(), "grandchild must be tombstoned by transitive cascade");
}

// 017.18: displacement cascade does not overwrite an existing tombstone on a descendant
#[test]
fn record_displaced_does_not_overwrite_existing_descendant_tombstone() {
    let db = tmp_db("displaced_no_overwrite");
    let store = make_store();
    store.initialize(&db).unwrap();
    let mod_time = "2024-06-01_00-00-00_000000Z";
    store.record_present(&db, "tree/dir", mod_time, -1).unwrap();
    store.record_present(&db, "tree/dir/already_gone.txt", mod_time, 200).unwrap();
    store.record_absent(&db, "tree/dir/already_gone.txt").unwrap();
    let before_cascade = store.read_row(&db, "tree/dir/already_gone.txt").unwrap().expect("row must exist");
    let original_deleted_time = before_cascade.deleted_time.clone().expect("must already be tombstoned");
    store.record_displaced(&db, "tree/dir").unwrap();
    let after_cascade = store.read_row(&db, "tree/dir/already_gone.txt").unwrap().expect("row must still exist");
    assert_eq!(
        after_cascade.deleted_time,
        Some(original_deleted_time),
        "cascade must not overwrite an existing tombstone on a descendant"
    );
}

// 018.2: prune keeps rows whose deleted_time is within the keep_del_days window
#[test]
fn prune_keeps_fresh_tombstone_within_window() {
    let db = tmp_db("prune_keeps_fresh");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "recent.txt", "2024-06-01_00-00-00_000000Z", 100).unwrap();
    store.record_absent(&db, "recent.txt").unwrap();
    store.prune(&db, 36500).unwrap();
    let row = store.read_row(&db, "recent.txt").unwrap();
    assert!(row.is_some(), "fresh tombstone must be kept when within the keep_del_days window");
}

// 018.2: prune keeps a fresh live row whose last_seen is within the keep_del_days window
#[test]
fn prune_keeps_fresh_live_row_within_window() {
    let db = tmp_db("prune_keeps_live");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "active.txt", "2024-06-01_00-00-00_000000Z", 512).unwrap();
    store.prune(&db, 36500).unwrap();
    let row = store.read_row(&db, "active.txt").unwrap();
    assert!(row.is_some(), "fresh live row must be kept when within the keep_del_days window");
}

// 018.6: prune returns Ok and does not fail the run
#[test]
fn prune_returns_ok() {
    let db = tmp_db("prune_ok");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.prune(&db, 30).expect("prune must return Ok");
}

// 024.6: dry-run -- Store creates and updates the local working database unchanged
#[test]
fn dry_run_creates_and_updates_local_db() {
    let db = tmp_db("dry_run");
    let store = make_store();
    store.initialize(&db).unwrap();
    store.record_present(&db, "a/b.txt", "2024-06-01_00-00-00_000000Z", 512).unwrap();
    let row = store.read_row(&db, "a/b.txt").unwrap().expect("row must exist after local-only update");
    assert_eq!(row.byte_size, 512);
}
