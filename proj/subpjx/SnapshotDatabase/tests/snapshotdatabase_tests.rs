use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use formatrules::FormatRules;
use peertransportsurface::ConnectedPeerRoot;
use rusqlite::Connection;
use snapshotdatabase::{
    new, SnapshotDatabase, SnapshotDatabaseCleanupRequest, SnapshotDatabaseCompletedCopyRequest,
    SnapshotDatabaseConfirmedAbsenceRequest, SnapshotDatabaseConfirmedFileRequest,
    SnapshotDatabaseCreatedDirectoryRequest, SnapshotDatabaseDiagnostic,
    SnapshotDatabaseDiagnosticKind, SnapshotDatabaseDiagnosticLevel,
    SnapshotDatabaseDisplacementRequest, SnapshotDatabaseEntryIdentity,
    SnapshotDatabaseIntendedCopyRequest, SnapshotDatabaseListedDirectoryRequest,
    SnapshotDatabaseListedFileRequest, SnapshotDatabasePeerDatabase,
    SnapshotDatabasePrepareRequest, SnapshotDatabasePrepareResult, SnapshotDatabaseRunMode,
    SnapshotDatabaseUploadRequest, SnapshotDatabaseUploadResult,
};

static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kitchensync_snapshotdatabase_{name}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&path);
        let _ = fs::remove_file(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn peer(&self) -> ConnectedPeerRoot {
        ConnectedPeerRoot {
            handle: Arc::new(self.path.clone()),
        }
    }

    fn child(&self, relative: &str) -> PathBuf {
        relative.split('/').fold(self.path.clone(), |path, part| {
            path.join(part)
        })
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_file(&self.path);
    }
}

struct Subject {
    snapshot_database: Arc<dyn SnapshotDatabase>,
    format_rules: Arc<dyn FormatRules>,
}

fn subject() -> Subject {
    let format_rules = formatrules::new();
    let peer_transport_surface = peertransportsurface::new();
    let snapshot_database = new(format_rules.clone(), peer_transport_surface);
    Subject {
        snapshot_database,
        format_rules,
    }
}

fn local_database(root: &TestRoot, name: &str) -> SnapshotDatabasePeerDatabase {
    SnapshotDatabasePeerDatabase {
        peer_index: 7,
        local_snapshot_path: root.child(name),
    }
}

fn identity(format_rules: &dyn FormatRules, relative_path: &str) -> SnapshotDatabaseEntryIdentity {
    let ids = format_rules.snapshot_path_ids(relative_path).unwrap();
    SnapshotDatabaseEntryIdentity {
        id: ids.id,
        parent_id: ids.parent_id,
        basename: relative_path.rsplit('/').next().unwrap().to_string(),
    }
}

fn create_database(subject: &dyn SnapshotDatabase, path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    subject.create_snapshot_database(path.to_path_buf()).unwrap();
}

fn listed_file(
    subject: &dyn SnapshotDatabase,
    database: SnapshotDatabasePeerDatabase,
    entry: SnapshotDatabaseEntryIdentity,
    mod_time: &str,
    byte_size: i64,
    last_seen: &str,
) {
    subject
        .record_listed_file(SnapshotDatabaseListedFileRequest {
            database,
            entry,
            mod_time: mod_time.to_string(),
            byte_size,
            last_seen: last_seen.to_string(),
        })
        .unwrap();
}

fn listed_directory(
    subject: &dyn SnapshotDatabase,
    database: SnapshotDatabasePeerDatabase,
    entry: SnapshotDatabaseEntryIdentity,
    mod_time: &str,
    last_seen: &str,
) {
    subject
        .record_listed_directory(SnapshotDatabaseListedDirectoryRequest {
            database,
            entry,
            mod_time: mod_time.to_string(),
            last_seen: last_seen.to_string(),
        })
        .unwrap();
}

fn read_row(
    subject: &dyn SnapshotDatabase,
    database: SnapshotDatabasePeerDatabase,
    entry_id: &str,
) -> snapshotdatabase::SnapshotDatabaseRow {
    subject
        .read_snapshot_row(database, entry_id.to_string())
        .unwrap()
        .expect("snapshot row should exist")
}

fn assert_missing(subject: &dyn SnapshotDatabase, database: SnapshotDatabasePeerDatabase, id: &str) {
    assert_eq!(
        subject
            .read_snapshot_row(database, id.to_string())
            .unwrap(),
        None
    );
}

fn live_snapshot_path(root: &TestRoot) -> PathBuf {
    root.child(".kitchensync/snapshot.db")
}

fn swap_new_path(root: &TestRoot) -> PathBuf {
    root.child(".kitchensync/SWAP/snapshot.db/new")
}

fn swap_old_path(root: &TestRoot) -> PathBuf {
    root.child(".kitchensync/SWAP/snapshot.db/old")
}

#[test]
fn create_snapshot_database_writes_the_specified_sqlite_schema() {
    let subject = subject();
    let root = TestRoot::new("schema");
    let path = root.child("snapshot.db");

    subject
        .snapshot_database
        .create_snapshot_database(path.clone())
        .unwrap();

    let connection = Connection::open(&path).unwrap();
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "delete");

    let tables: Vec<String> = connection
        .prepare(
            "SELECT name FROM sqlite_schema \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(tables, vec!["snapshot"]);

    let views: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'view' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(views, Vec::<String>::new());

    let columns: Vec<(String, String, bool, bool)> = connection
        .prepare("PRAGMA table_info('snapshot')")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
                row.get::<_, i64>(5)? != 0,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        vec![
            ("id".to_string(), "TEXT".to_string(), false, true),
            ("parent_id".to_string(), "TEXT".to_string(), false, false),
            ("basename".to_string(), "TEXT".to_string(), true, false),
            ("mod_time".to_string(), "TEXT".to_string(), true, false),
            ("byte_size".to_string(), "INTEGER".to_string(), true, false),
            ("last_seen".to_string(), "TEXT".to_string(), false, false),
            ("deleted_time".to_string(), "TEXT".to_string(), false, false),
        ]
    );

    let mut indexes = BTreeSet::new();
    let mut index_rows = connection.prepare("PRAGMA index_list('snapshot')").unwrap();
    let index_names: Vec<(String, String)> = index_rows
        .query_map([], |row| Ok((row.get(1)?, row.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (name, origin) in index_names {
        if origin == "pk" {
            continue;
        }
        let pragma = format!("PRAGMA index_info('{name}')");
        let columns: Vec<String> = connection
            .prepare(&pragma)
            .unwrap()
            .query_map([], |row| row.get(2))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        indexes.insert(columns);
    }
    assert!(indexes.contains(&vec!["parent_id".to_string()]));
    assert!(indexes.contains(&vec!["last_seen".to_string()]));
    assert!(indexes.contains(&vec!["deleted_time".to_string()]));

    drop(index_rows);
    drop(connection);
    assert!(!root.child("snapshot.db-journal").exists());
    assert!(!root.child("snapshot.db-wal").exists());
    assert!(!root.child("snapshot.db-shm").exists());
}

#[test]
fn read_snapshot_row_returns_none_for_a_missing_entry() {
    let subject = subject();
    let root = TestRoot::new("read_missing");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);

    assert_missing(
        subject.snapshot_database.as_ref(),
        database,
        "missing-entry-id",
    );
}

#[test]
fn present_file_and_directory_records_store_supplied_snapshot_values() {
    let subject = subject();
    let root = TestRoot::new("present_records");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);
    let file = identity(subject.format_rules.as_ref(), "docs/readme.txt");
    let directory = identity(subject.format_rules.as_ref(), "docs");

    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        file.clone(),
        "2024-01-01_00-00-00_000001Z",
        42,
        "2024-01-01_00-00-01_000001Z",
    );
    listed_directory(
        subject.snapshot_database.as_ref(),
        database.clone(),
        directory.clone(),
        "2024-01-01_00-00-02_000001Z",
        "2024-01-01_00-00-03_000001Z",
    );

    let file_row = read_row(subject.snapshot_database.as_ref(), database.clone(), &file.id);
    assert_eq!(file_row.parent_id, Some(file.parent_id));
    assert_eq!(file_row.basename, "readme.txt");
    assert_eq!(file_row.mod_time, "2024-01-01_00-00-00_000001Z");
    assert_eq!(file_row.byte_size, 42);
    assert_eq!(
        file_row.last_seen,
        Some("2024-01-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(file_row.deleted_time, None);

    let directory_row = read_row(subject.snapshot_database.as_ref(), database, &directory.id);
    assert_eq!(directory_row.parent_id, Some(directory.parent_id));
    assert_eq!(directory_row.basename, "docs");
    assert_eq!(directory_row.mod_time, "2024-01-01_00-00-02_000001Z");
    assert_eq!(directory_row.byte_size, -1);
    assert_eq!(
        directory_row.last_seen,
        Some("2024-01-01_00-00-03_000001Z".to_string())
    );
    assert_eq!(directory_row.deleted_time, None);
}

#[test]
fn confirmed_file_records_the_winning_file_state_as_present() {
    let subject = subject();
    let root = TestRoot::new("confirmed_file");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);
    let entry = identity(subject.format_rules.as_ref(), "docs/readme.txt");

    subject
        .snapshot_database
        .record_confirmed_file(SnapshotDatabaseConfirmedFileRequest {
            database: database.clone(),
            entry: entry.clone(),
            mod_time: "2024-02-01_00-00-00_000001Z".to_string(),
            byte_size: 101,
            last_seen: "2024-02-01_00-00-01_000001Z".to_string(),
        })
        .unwrap();

    let row = read_row(subject.snapshot_database.as_ref(), database, &entry.id);
    assert_eq!(row.mod_time, "2024-02-01_00-00-00_000001Z");
    assert_eq!(row.byte_size, 101);
    assert_eq!(
        row.last_seen,
        Some("2024-02-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(row.deleted_time, None);
}

#[test]
fn intended_copy_rows_preserve_last_seen_until_copy_completion() {
    let subject = subject();
    let root = TestRoot::new("intended_copy");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);
    let new_entry = identity(subject.format_rules.as_ref(), "docs/new.txt");
    let reused_entry = identity(subject.format_rules.as_ref(), "docs/reused.txt");

    subject
        .snapshot_database
        .record_intended_file_copy(SnapshotDatabaseIntendedCopyRequest {
            database: database.clone(),
            entry: new_entry.clone(),
            mod_time: "2024-03-01_00-00-00_000001Z".to_string(),
            byte_size: 55,
        })
        .unwrap();
    let new_row = read_row(subject.snapshot_database.as_ref(), database.clone(), &new_entry.id);
    assert_eq!(new_row.last_seen, None);
    assert_eq!(new_row.deleted_time, None);

    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        reused_entry.clone(),
        "2024-03-01_00-00-01_000001Z",
        10,
        "2024-03-01_00-00-02_000001Z",
    );
    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database: database.clone(),
            entry_id: reused_entry.id.clone(),
        })
        .unwrap();
    subject
        .snapshot_database
        .record_intended_file_copy(SnapshotDatabaseIntendedCopyRequest {
            database: database.clone(),
            entry: reused_entry.clone(),
            mod_time: "2024-03-01_00-00-03_000001Z".to_string(),
            byte_size: 66,
        })
        .unwrap();
    let reused_before_completion =
        read_row(subject.snapshot_database.as_ref(), database.clone(), &reused_entry.id);
    assert_eq!(
        reused_before_completion.last_seen,
        Some("2024-03-01_00-00-02_000001Z".to_string())
    );
    assert_eq!(reused_before_completion.deleted_time, None);

    subject
        .snapshot_database
        .record_completed_file_copy(SnapshotDatabaseCompletedCopyRequest {
            database: database.clone(),
            entry_id: reused_entry.id.clone(),
            last_seen: "2024-03-01_00-00-04_000001Z".to_string(),
        })
        .unwrap();
    let completed = read_row(subject.snapshot_database.as_ref(), database, &reused_entry.id);
    assert_eq!(completed.mod_time, "2024-03-01_00-00-03_000001Z");
    assert_eq!(completed.byte_size, 66);
    assert_eq!(
        completed.last_seen,
        Some("2024-03-01_00-00-04_000001Z".to_string())
    );
    assert_eq!(completed.deleted_time, None);
}

#[test]
fn created_directory_records_a_present_directory() {
    let subject = subject();
    let root = TestRoot::new("created_directory");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);
    let entry = identity(subject.format_rules.as_ref(), "created");

    subject
        .snapshot_database
        .record_created_directory(SnapshotDatabaseCreatedDirectoryRequest {
            database: database.clone(),
            entry: entry.clone(),
            mod_time: "2024-04-01_00-00-00_000001Z".to_string(),
            last_seen: "2024-04-01_00-00-01_000001Z".to_string(),
        })
        .unwrap();

    let row = read_row(subject.snapshot_database.as_ref(), database, &entry.id);
    assert_eq!(row.mod_time, "2024-04-01_00-00-00_000001Z");
    assert_eq!(row.byte_size, -1);
    assert_eq!(
        row.last_seen,
        Some("2024-04-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(row.deleted_time, None);
}

#[test]
fn confirmed_absence_tombstones_only_untombstoned_existing_rows() {
    let subject = subject();
    let root = TestRoot::new("absence");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);
    let entry = identity(subject.format_rules.as_ref(), "docs/missing.txt");

    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        entry.clone(),
        "2024-05-01_00-00-00_000001Z",
        12,
        "2024-05-01_00-00-01_000001Z",
    );
    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database: database.clone(),
            entry_id: entry.id.clone(),
        })
        .unwrap();
    let tombstoned = read_row(subject.snapshot_database.as_ref(), database.clone(), &entry.id);
    assert_eq!(
        tombstoned.last_seen,
        Some("2024-05-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(
        tombstoned.deleted_time,
        Some("2024-05-01_00-00-01_000001Z".to_string())
    );

    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database: database.clone(),
            entry_id: entry.id.clone(),
        })
        .unwrap();
    assert_eq!(
        read_row(subject.snapshot_database.as_ref(), database.clone(), &entry.id),
        tombstoned
    );

    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database,
            entry_id: "missing-entry-id".to_string(),
        })
        .unwrap();
}

#[test]
fn directory_displacement_cascades_only_to_untombstoned_descendants_on_the_same_peer() {
    let subject = subject();
    let peer_a = TestRoot::new("displacement_a");
    let peer_b = TestRoot::new("displacement_b");
    let database_a = local_database(&peer_a, "snapshot.db");
    let database_b = local_database(&peer_b, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database_a.local_snapshot_path);
    create_database(subject.snapshot_database.as_ref(), &database_b.local_snapshot_path);
    let dir = identity(subject.format_rules.as_ref(), "docs");
    let child = identity(subject.format_rules.as_ref(), "docs/live.txt");
    let subdir = identity(subject.format_rules.as_ref(), "docs/sub");
    let nested = identity(subject.format_rules.as_ref(), "docs/sub/already.txt");
    let outside = identity(subject.format_rules.as_ref(), "other.txt");

    listed_directory(
        subject.snapshot_database.as_ref(),
        database_a.clone(),
        dir.clone(),
        "2024-06-01_00-00-00_000001Z",
        "2024-06-01_00-00-01_000001Z",
    );
    listed_file(
        subject.snapshot_database.as_ref(),
        database_a.clone(),
        child.clone(),
        "2024-06-01_00-00-02_000001Z",
        21,
        "2024-06-01_00-00-03_000001Z",
    );
    listed_directory(
        subject.snapshot_database.as_ref(),
        database_a.clone(),
        subdir.clone(),
        "2024-06-01_00-00-04_000001Z",
        "2024-06-01_00-00-05_000001Z",
    );
    listed_file(
        subject.snapshot_database.as_ref(),
        database_a.clone(),
        nested.clone(),
        "2024-06-01_00-00-06_000001Z",
        22,
        "2024-06-01_00-00-07_000001Z",
    );
    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database: database_a.clone(),
            entry_id: nested.id.clone(),
        })
        .unwrap();
    listed_file(
        subject.snapshot_database.as_ref(),
        database_a.clone(),
        outside.clone(),
        "2024-06-01_00-00-08_000001Z",
        23,
        "2024-06-01_00-00-09_000001Z",
    );
    listed_file(
        subject.snapshot_database.as_ref(),
        database_b.clone(),
        child.clone(),
        "2024-06-01_00-00-10_000001Z",
        24,
        "2024-06-01_00-00-11_000001Z",
    );

    let nested_before = read_row(subject.snapshot_database.as_ref(), database_a.clone(), &nested.id);
    let outside_before =
        read_row(subject.snapshot_database.as_ref(), database_a.clone(), &outside.id);
    let peer_b_before = read_row(subject.snapshot_database.as_ref(), database_b.clone(), &child.id);
    subject
        .snapshot_database
        .record_successful_displacement(SnapshotDatabaseDisplacementRequest {
            database: database_a.clone(),
            entry_id: dir.id.clone(),
            is_directory: true,
        })
        .unwrap();

    let dir_after = read_row(subject.snapshot_database.as_ref(), database_a.clone(), &dir.id);
    let child_after = read_row(subject.snapshot_database.as_ref(), database_a.clone(), &child.id);
    let subdir_after = read_row(subject.snapshot_database.as_ref(), database_a.clone(), &subdir.id);
    assert_eq!(
        dir_after.deleted_time,
        Some("2024-06-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(
        child_after.deleted_time,
        Some("2024-06-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(
        subdir_after.deleted_time,
        Some("2024-06-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(
        read_row(subject.snapshot_database.as_ref(), database_a.clone(), &nested.id),
        nested_before
    );
    assert_eq!(
        read_row(subject.snapshot_database.as_ref(), database_a, &outside.id),
        outside_before
    );
    assert_eq!(
        read_row(subject.snapshot_database.as_ref(), database_b, &child.id),
        peer_b_before
    );
}

#[test]
fn file_displacement_tombstones_only_the_displaced_row() {
    let subject = subject();
    let root = TestRoot::new("file_displacement");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);
    let displaced = identity(subject.format_rules.as_ref(), "docs/file.txt");
    let sibling = identity(subject.format_rules.as_ref(), "docs/sibling.txt");

    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        displaced.clone(),
        "2024-07-01_00-00-00_000001Z",
        31,
        "2024-07-01_00-00-01_000001Z",
    );
    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        sibling.clone(),
        "2024-07-01_00-00-02_000001Z",
        32,
        "2024-07-01_00-00-03_000001Z",
    );
    let sibling_before =
        read_row(subject.snapshot_database.as_ref(), database.clone(), &sibling.id);

    subject
        .snapshot_database
        .record_successful_displacement(SnapshotDatabaseDisplacementRequest {
            database: database.clone(),
            entry_id: displaced.id.clone(),
            is_directory: false,
        })
        .unwrap();

    let displaced_after =
        read_row(subject.snapshot_database.as_ref(), database.clone(), &displaced.id);
    assert_eq!(
        displaced_after.deleted_time,
        Some("2024-07-01_00-00-01_000001Z".to_string())
    );
    assert_eq!(
        read_row(subject.snapshot_database.as_ref(), database, &sibling.id),
        sibling_before
    );
}

#[test]
fn cleanup_removes_only_old_tombstones_and_requested_old_stale_rows() {
    let subject = subject();
    let root = TestRoot::new("cleanup");
    let database = local_database(&root, "snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &database.local_snapshot_path);
    let old_tombstone = identity(subject.format_rules.as_ref(), "old-tombstone.txt");
    let kept_tombstone = identity(subject.format_rules.as_ref(), "kept-tombstone.txt");
    let old_stale = identity(subject.format_rules.as_ref(), "old-stale.txt");
    let null_stale = identity(subject.format_rules.as_ref(), "null-stale.txt");
    let not_obsolete = identity(subject.format_rules.as_ref(), "not-obsolete.txt");
    let fresh_obsolete = identity(subject.format_rules.as_ref(), "fresh-obsolete.txt");

    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        old_tombstone.clone(),
        "2024-08-01_00-00-00_000001Z",
        1,
        "2024-08-01_00-00-01_000001Z",
    );
    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database: database.clone(),
            entry_id: old_tombstone.id.clone(),
        })
        .unwrap();
    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        kept_tombstone.clone(),
        "2024-08-03_00-00-00_000001Z",
        2,
        "2024-08-03_00-00-01_000001Z",
    );
    subject
        .snapshot_database
        .record_confirmed_absence(SnapshotDatabaseConfirmedAbsenceRequest {
            database: database.clone(),
            entry_id: kept_tombstone.id.clone(),
        })
        .unwrap();
    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        old_stale.clone(),
        "2024-08-01_00-00-02_000001Z",
        3,
        "2024-08-01_00-00-03_000001Z",
    );
    subject
        .snapshot_database
        .record_intended_file_copy(SnapshotDatabaseIntendedCopyRequest {
            database: database.clone(),
            entry: null_stale.clone(),
            mod_time: "2024-08-01_00-00-04_000001Z".to_string(),
            byte_size: 4,
        })
        .unwrap();
    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        not_obsolete.clone(),
        "2024-08-01_00-00-05_000001Z",
        5,
        "2024-08-01_00-00-06_000001Z",
    );
    listed_file(
        subject.snapshot_database.as_ref(),
        database.clone(),
        fresh_obsolete.clone(),
        "2024-08-03_00-00-02_000001Z",
        6,
        "2024-08-03_00-00-03_000001Z",
    );

    let result = subject
        .snapshot_database
        .cleanup_snapshot_rows(SnapshotDatabaseCleanupRequest {
            database: database.clone(),
            older_than_timestamp: "2024-08-02_00-00-00_000001Z".to_string(),
            obsolete_untombstoned_ids: vec![
                old_stale.id.clone(),
                null_stale.id.clone(),
                fresh_obsolete.id.clone(),
            ],
        })
        .unwrap();

    assert_eq!(result.removed_tombstone_rows, 1);
    assert_eq!(result.removed_stale_rows, 2);
    assert_missing(
        subject.snapshot_database.as_ref(),
        database.clone(),
        &old_tombstone.id,
    );
    assert_missing(subject.snapshot_database.as_ref(), database.clone(), &old_stale.id);
    assert_missing(subject.snapshot_database.as_ref(), database.clone(), &null_stale.id);
    assert!(subject
        .snapshot_database
        .read_snapshot_row(database.clone(), kept_tombstone.id)
        .unwrap()
        .is_some());
    assert!(subject
        .snapshot_database
        .read_snapshot_row(database.clone(), not_obsolete.id)
        .unwrap()
        .is_some());
    assert!(subject
        .snapshot_database
        .read_snapshot_row(database, fresh_obsolete.id)
        .unwrap()
        .is_some());
}

#[test]
fn prepare_missing_live_snapshot_creates_empty_local_database_and_ignores_sidecars() {
    let subject = subject();
    let peer_root = TestRoot::new("prepare_missing_live");
    let local_root = TestRoot::new("prepare_missing_local");
    fs::create_dir_all(peer_root.child(".kitchensync")).unwrap();
    fs::write(peer_root.child(".kitchensync/snapshot.db-wal"), b"sidecar").unwrap();
    let local_snapshot_path = local_root.child("uuid/snapshot.db");

    let result = subject
        .snapshot_database
        .prepare_peer_snapshot(SnapshotDatabasePrepareRequest {
            peer_index: 3,
            peer: peer_root.peer(),
            local_snapshot_path: local_snapshot_path.clone(),
            mode: SnapshotDatabaseRunMode::Normal,
        });

    assert_eq!(
        result,
        SnapshotDatabasePrepareResult::Prepared(snapshotdatabase::SnapshotDatabasePreparedPeer {
            peer_index: 3,
            local_snapshot_path: local_snapshot_path.clone(),
            had_snapshot_history: false,
        })
    );
    assert!(local_snapshot_path.exists());
    let local_database = SnapshotDatabasePeerDatabase {
        peer_index: 3,
        local_snapshot_path,
    };
    assert_missing(subject.snapshot_database.as_ref(), local_database, "missing-row");
    assert!(!live_snapshot_path(&peer_root).exists());
    assert!(peer_root.child(".kitchensync/snapshot.db-wal").exists());
}

#[test]
fn prepare_existing_live_snapshot_downloads_that_peer_database() {
    let subject = subject();
    let peer_root = TestRoot::new("prepare_existing_peer");
    let local_root = TestRoot::new("prepare_existing_local");
    let live_path = live_snapshot_path(&peer_root);
    create_database(subject.snapshot_database.as_ref(), &live_path);
    let live_database = SnapshotDatabasePeerDatabase {
        peer_index: 4,
        local_snapshot_path: live_path,
    };
    let entry = identity(subject.format_rules.as_ref(), "docs/readme.txt");
    listed_file(
        subject.snapshot_database.as_ref(),
        live_database,
        entry.clone(),
        "2024-09-01_00-00-00_000001Z",
        88,
        "2024-09-01_00-00-01_000001Z",
    );
    let local_snapshot_path = local_root.child("uuid/snapshot.db");

    let result = subject
        .snapshot_database
        .prepare_peer_snapshot(SnapshotDatabasePrepareRequest {
            peer_index: 4,
            peer: peer_root.peer(),
            local_snapshot_path: local_snapshot_path.clone(),
            mode: SnapshotDatabaseRunMode::Normal,
        });

    assert_eq!(
        result,
        SnapshotDatabasePrepareResult::Prepared(snapshotdatabase::SnapshotDatabasePreparedPeer {
            peer_index: 4,
            local_snapshot_path: local_snapshot_path.clone(),
            had_snapshot_history: true,
        })
    );
    let downloaded = read_row(
        subject.snapshot_database.as_ref(),
        SnapshotDatabasePeerDatabase {
            peer_index: 4,
            local_snapshot_path,
        },
        &entry.id,
    );
    assert_eq!(downloaded.byte_size, 88);
}

#[test]
fn normal_prepare_recovers_documented_snapshot_swap_states() {
    let subject = subject();

    for (name, live, new, old, expected_live, expected_new, expected_old) in [
        ("old_live_new", true, true, true, true, false, false),
        ("old_new", false, true, true, true, false, false),
        ("old_only", false, false, true, true, false, false),
        ("new_live", true, true, false, true, false, false),
        ("new_only", false, true, false, true, false, false),
    ] {
        let peer_root = TestRoot::new(name);
        let local_root = TestRoot::new(&format!("{name}_local"));
        if live {
            create_database(subject.snapshot_database.as_ref(), &live_snapshot_path(&peer_root));
        }
        if new {
            create_database(subject.snapshot_database.as_ref(), &swap_new_path(&peer_root));
        }
        if old {
            create_database(subject.snapshot_database.as_ref(), &swap_old_path(&peer_root));
        }

        let result = subject
            .snapshot_database
            .prepare_peer_snapshot(SnapshotDatabasePrepareRequest {
                peer_index: 5,
                peer: peer_root.peer(),
                local_snapshot_path: local_root.child("uuid/snapshot.db"),
                mode: SnapshotDatabaseRunMode::Normal,
            });

        assert!(
            matches!(result, SnapshotDatabasePrepareResult::Prepared(_)),
            "{name}"
        );
        assert_eq!(live_snapshot_path(&peer_root).exists(), expected_live, "{name}");
        assert_eq!(swap_new_path(&peer_root).exists(), expected_new, "{name}");
        assert_eq!(swap_old_path(&peer_root).exists(), expected_old, "{name}");
    }
}

#[test]
fn dry_run_prepare_downloads_live_snapshot_without_recovering_swap_state() {
    let subject = subject();
    let peer_root = TestRoot::new("dry_run_prepare");
    let local_root = TestRoot::new("dry_run_prepare_local");
    create_database(subject.snapshot_database.as_ref(), &live_snapshot_path(&peer_root));
    create_database(subject.snapshot_database.as_ref(), &swap_new_path(&peer_root));
    create_database(subject.snapshot_database.as_ref(), &swap_old_path(&peer_root));

    let result = subject
        .snapshot_database
        .prepare_peer_snapshot(SnapshotDatabasePrepareRequest {
            peer_index: 6,
            peer: peer_root.peer(),
            local_snapshot_path: local_root.child("uuid/snapshot.db"),
            mode: SnapshotDatabaseRunMode::DryRun,
        });

    assert!(matches!(result, SnapshotDatabasePrepareResult::Prepared(_)));
    assert!(live_snapshot_path(&peer_root).exists());
    assert!(swap_new_path(&peer_root).exists());
    assert!(swap_old_path(&peer_root).exists());
}

#[test]
fn prepare_failure_excludes_only_that_peer_with_startup_error_diagnostic() {
    let subject = subject();
    let peer_root = TestRoot::new("prepare_failure");
    let local_root = TestRoot::new("prepare_failure_local");
    fs::create_dir_all(live_snapshot_path(&peer_root)).unwrap();

    let result = subject
        .snapshot_database
        .prepare_peer_snapshot(SnapshotDatabasePrepareRequest {
            peer_index: 9,
            peer: peer_root.peer(),
            local_snapshot_path: local_root.child("uuid/snapshot.db"),
            mode: SnapshotDatabaseRunMode::Normal,
        });

    assert_eq!(
        result,
        SnapshotDatabasePrepareResult::Excluded(SnapshotDatabaseDiagnostic {
            level: SnapshotDatabaseDiagnosticLevel::Error,
            peer_index: 9,
            kind: SnapshotDatabaseDiagnosticKind::SnapshotStartupFailed,
        })
    );
}

#[test]
fn upload_publishes_only_the_closed_snapshot_file_through_swap_replacement() {
    let subject = subject();
    let peer_root = TestRoot::new("upload_peer");
    let local_root = TestRoot::new("upload_local");
    let old_live = live_snapshot_path(&peer_root);
    create_database(subject.snapshot_database.as_ref(), &old_live);
    let local_snapshot_path = local_root.child("snapshot.db");
    create_database(subject.snapshot_database.as_ref(), &local_snapshot_path);
    let entry = identity(subject.format_rules.as_ref(), "docs/uploaded.txt");
    listed_file(
        subject.snapshot_database.as_ref(),
        SnapshotDatabasePeerDatabase {
            peer_index: 10,
            local_snapshot_path: local_snapshot_path.clone(),
        },
        entry.clone(),
        "2024-10-01_00-00-00_000001Z",
        123,
        "2024-10-01_00-00-01_000001Z",
    );

    let result = subject
        .snapshot_database
        .upload_snapshot(SnapshotDatabaseUploadRequest {
            peer_index: 10,
            peer: peer_root.peer(),
            local_snapshot_path: local_snapshot_path.clone(),
        });

    assert_eq!(result, SnapshotDatabaseUploadResult::Uploaded);
    assert!(live_snapshot_path(&peer_root).exists());
    assert!(!swap_new_path(&peer_root).exists());
    assert!(!swap_old_path(&peer_root).exists());
    assert!(!peer_root.child(".kitchensync/snapshot.db-journal").exists());
    assert!(!peer_root.child(".kitchensync/snapshot.db-wal").exists());
    assert!(!peer_root.child(".kitchensync/snapshot.db-shm").exists());
    let uploaded = read_row(
        subject.snapshot_database.as_ref(),
        SnapshotDatabasePeerDatabase {
            peer_index: 10,
            local_snapshot_path: live_snapshot_path(&peer_root),
        },
        &entry.id,
    );
    assert_eq!(uploaded.byte_size, 123);
}

#[test]
fn upload_failure_before_swap_old_returns_error_diagnostic() {
    let subject = subject();
    let peer_root = TestRoot::new("upload_missing_local");

    let result = subject
        .snapshot_database
        .upload_snapshot(SnapshotDatabaseUploadRequest {
            peer_index: 11,
            peer: peer_root.peer(),
            local_snapshot_path: peer_root.child("missing-local-snapshot.db"),
        });

    assert_eq!(
        result,
        SnapshotDatabaseUploadResult::Failed(SnapshotDatabaseDiagnostic {
            level: SnapshotDatabaseDiagnosticLevel::Error,
            peer_index: 11,
            kind: SnapshotDatabaseDiagnosticKind::SnapshotUploadFailedBeforeSwapOld,
        })
    );
    assert!(!swap_old_path(&peer_root).exists());
}
