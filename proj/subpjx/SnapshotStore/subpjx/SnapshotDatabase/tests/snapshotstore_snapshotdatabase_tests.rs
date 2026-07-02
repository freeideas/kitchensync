use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use snapshotstore_snapshotdatabase::{
    new, SnapshotDatabase, SnapshotDatabaseErrorKind, SnapshotRow, SnapshotRowFacts,
    SnapshotRowIdentity,
};

const ROOT_PARENT_ID: &str = "JyBskcNRrBK";

fn subject() -> Arc<dyn SnapshotDatabase> {
    new(
        snapshotstore_snapshotdatabase_snapshotcleanup::new(),
        snapshotstore_snapshotdatabase_snapshotfile::new(),
        snapshotstore_snapshotdatabase_snapshotrows::new(),
    )
}

fn temp_snapshot_path(test_name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "snapshotdatabase-{test_name}-{}-{stamp}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).expect("create test temp directory");
    dir.join("snapshot.db")
}

fn identity(id: &str, parent_id: &str, basename: &str) -> SnapshotRowIdentity {
    SnapshotRowIdentity {
        id: id.to_string(),
        parent_id: parent_id.to_string(),
        basename: basename.to_string(),
    }
}

fn file_facts(
    id: &str,
    parent_id: &str,
    basename: &str,
    mod_time: &str,
    byte_size: i64,
) -> SnapshotRowFacts {
    SnapshotRowFacts {
        identity: identity(id, parent_id, basename),
        mod_time: mod_time.to_string(),
        byte_size,
    }
}

fn assert_row(
    row: SnapshotRow,
    id: &str,
    parent_id: &str,
    basename: &str,
    mod_time: &str,
    byte_size: i64,
    last_seen: Option<&str>,
    deleted_time: Option<&str>,
) {
    assert_eq!(row.identity, identity(id, parent_id, basename));
    assert_eq!(row.mod_time.as_str(), mod_time);
    assert_eq!(row.byte_size, byte_size);
    assert_eq!(row.last_seen.as_deref(), last_seen);
    assert_eq!(row.deleted_time.as_deref(), deleted_time);
}

fn schema_names(connection: &Connection, schema_type: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT name
             FROM sqlite_schema
             WHERE type = ?1
             AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .expect("prepare schema name query");
    statement
        .query_map([schema_type], |row| row.get::<_, String>(0))
        .expect("query schema names")
        .map(|row| row.expect("read schema name"))
        .collect()
}

#[test]
fn row_mutations_reject_root_or_missing_basename_identities() {
    let database = subject();
    let path = temp_snapshot_path("invalid-identities");
    let handle = database.create_empty(&path).expect("create snapshot database");

    let root_facts = SnapshotRowFacts {
        identity: identity(ROOT_PARENT_ID, ROOT_PARENT_ID, "."),
        mod_time: "2026-01-01T00:00:00Z".to_string(),
        byte_size: -1,
    };
    let error = database
        .confirm_present(&handle, &root_facts, "2026-01-01T00:01:00Z")
        .expect_err("sync root identity is rejected");
    assert_eq!(error.kind, SnapshotDatabaseErrorKind::InvalidRowIdentity);

    let missing_basename = SnapshotRowFacts {
        identity: identity("file", ROOT_PARENT_ID, ""),
        mod_time: "2026-01-01T00:00:00Z".to_string(),
        byte_size: 1,
    };
    let error = database
        .record_intended_file_copy(&handle, &missing_basename)
        .expect_err("missing basename is rejected");
    assert_eq!(error.kind, SnapshotDatabaseErrorKind::InvalidRowIdentity);

    assert_eq!(
        database
            .list_child_rows(&handle, ROOT_PARENT_ID)
            .expect("list rows after rejected mutations"),
        Vec::<SnapshotRow>::new()
    );
}

#[test]
fn create_empty_prepares_a_self_contained_rollback_journal_database_with_required_schema() {
    let database = subject();
    let path = temp_snapshot_path("create-empty-schema");

    let handle = database.create_empty(&path).expect("create snapshot database");
    assert_eq!(
        database
            .list_child_rows(&handle, ROOT_PARENT_ID)
            .expect("list rows below sync root"),
        Vec::<SnapshotRow>::new()
    );

    let prepared_path = database
        .prepare_for_upload(handle)
        .expect("prepare snapshot database for upload");
    assert_eq!(prepared_path, path);
    assert!(prepared_path.exists());
    assert!(!prepared_path.with_extension("db-wal").exists());
    assert!(!prepared_path.with_extension("db-shm").exists());
    assert!(!prepared_path.with_extension("db-journal").exists());

    let connection = Connection::open(&prepared_path).expect("open prepared snapshot database");
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("read journal mode");
    assert!(journal_mode.eq_ignore_ascii_case("delete"));

    assert_eq!(
        schema_names(&connection, "table"),
        vec!["snapshot".to_string()]
    );
    assert_eq!(schema_names(&connection, "view"), Vec::<String>::new());

    let columns: Vec<(String, String, i64, i64)> = connection
        .prepare("PRAGMA table_info(snapshot)")
        .expect("prepare table info")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .expect("query table info")
        .map(|row| row.expect("read table info"))
        .collect();
    assert_eq!(
        columns,
        vec![
            ("id".to_string(), "TEXT".to_string(), 0, 1),
            ("parent_id".to_string(), "TEXT".to_string(), 0, 0),
            ("basename".to_string(), "TEXT".to_string(), 1, 0),
            ("mod_time".to_string(), "TEXT".to_string(), 1, 0),
            ("byte_size".to_string(), "INTEGER".to_string(), 1, 0),
            ("last_seen".to_string(), "TEXT".to_string(), 0, 0),
            ("deleted_time".to_string(), "TEXT".to_string(), 0, 0),
        ]
    );

    let indexed_columns: BTreeSet<String> = connection
        .prepare("PRAGMA index_list(snapshot)")
        .expect("prepare index list")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query index list")
        .map(|name| {
            let name = name.expect("read index name");
            let sql = format!("PRAGMA index_info(\"{}\")", name.replace('"', "\"\""));
            let mut statement = connection.prepare(&sql).expect("prepare index info");
            statement
                .query_map([], |row| row.get::<_, String>(2))
                .expect("query index info")
                .map(|column| column.expect("read indexed column"))
                .collect::<Vec<_>>()
        })
        .filter_map(|columns| match columns.as_slice() {
            [column] => Some(column.clone()),
            _ => None,
        })
        .collect();
    assert!(indexed_columns.contains("parent_id"));
    assert!(indexed_columns.contains("last_seen"));
    assert!(indexed_columns.contains("deleted_time"));
}

#[test]
fn row_operations_record_presence_pending_copies_completion_and_tombstones() {
    let database = subject();
    let path = temp_snapshot_path("row-operations");
    let handle = database.create_empty(&path).expect("create snapshot database");

    let original = file_facts(
        "file-a",
        ROOT_PARENT_ID,
        "file.txt",
        "2026-01-02T03:04:05Z",
        17,
    );
    database
        .confirm_present(&handle, &original, "2026-01-02T03:10:00Z")
        .expect("confirm file present");
    assert_row(
        database
            .lookup_row(&handle, "file-a")
            .expect("lookup confirmed file")
            .expect("confirmed file row exists"),
        "file-a",
        ROOT_PARENT_ID,
        "file.txt",
        "2026-01-02T03:04:05Z",
        17,
        Some("2026-01-02T03:10:00Z"),
        None,
    );

    let replacement = file_facts(
        "file-a",
        ROOT_PARENT_ID,
        "file.txt",
        "2026-01-03T03:04:05Z",
        23,
    );
    database
        .record_intended_file_copy(&handle, &replacement)
        .expect("record intended copy over existing row");
    assert_row(
        database
            .lookup_row(&handle, "file-a")
            .expect("lookup pending copy")
            .expect("pending copy row exists"),
        "file-a",
        ROOT_PARENT_ID,
        "file.txt",
        "2026-01-03T03:04:05Z",
        23,
        Some("2026-01-02T03:10:00Z"),
        None,
    );

    let new_destination = file_facts(
        "file-b",
        ROOT_PARENT_ID,
        "copy.txt",
        "2026-01-04T03:04:05Z",
        99,
    );
    database
        .record_intended_file_copy(&handle, &new_destination)
        .expect("record intended copy for new row");
    assert_row(
        database
            .lookup_row(&handle, "file-b")
            .expect("lookup new pending copy")
            .expect("new pending copy row exists"),
        "file-b",
        ROOT_PARENT_ID,
        "copy.txt",
        "2026-01-04T03:04:05Z",
        99,
        None,
        None,
    );

    database
        .complete_file_copy(
            &handle,
            &identity("file-b", ROOT_PARENT_ID, "copy.txt"),
            "2026-01-04T03:20:00Z",
        )
        .expect("complete file copy");
    assert_eq!(
        database
            .lookup_row(&handle, "file-b")
            .expect("lookup completed copy")
            .expect("completed copy row exists")
            .last_seen
            .as_deref(),
        Some("2026-01-04T03:20:00Z")
    );

    database
        .complete_directory_creation(
            &handle,
            &identity("dir-a", ROOT_PARENT_ID, "folder"),
            "2026-01-05T03:04:05Z",
            "2026-01-05T03:20:00Z",
        )
        .expect("complete directory creation");
    assert_row(
        database
            .lookup_row(&handle, "dir-a")
            .expect("lookup completed directory")
            .expect("completed directory row exists"),
        "dir-a",
        ROOT_PARENT_ID,
        "folder",
        "2026-01-05T03:04:05Z",
        -1,
        Some("2026-01-05T03:20:00Z"),
        None,
    );

    database
        .confirm_absent(&handle, &identity("file-a", ROOT_PARENT_ID, "file.txt"))
        .expect("confirm file absent");
    assert_row(
        database
            .lookup_row(&handle, "file-a")
            .expect("lookup absent file")
            .expect("absent file tombstone remains"),
        "file-a",
        ROOT_PARENT_ID,
        "file.txt",
        "2026-01-03T03:04:05Z",
        23,
        Some("2026-01-02T03:10:00Z"),
        Some("2026-01-02T03:10:00Z"),
    );

    database
        .confirm_absent(&handle, &identity("file-a", ROOT_PARENT_ID, "file.txt"))
        .expect("confirm already tombstoned file absent");
    assert_eq!(
        database
            .lookup_row(&handle, "file-a")
            .expect("lookup unchanged tombstone")
            .expect("tombstone remains")
            .deleted_time
            .as_deref(),
        Some("2026-01-02T03:10:00Z")
    );
}

#[test]
fn directory_displacement_cascade_and_cleanup_are_scoped_to_one_local_database() {
    let database = subject();
    let first_path = temp_snapshot_path("cascade-first");
    let second_path = temp_snapshot_path("cascade-second");
    let first = database
        .create_empty(&first_path)
        .expect("create first snapshot database");
    let second = database
        .create_empty(&second_path)
        .expect("create second snapshot database");

    database
        .complete_directory_creation(
            &first,
            &identity("dir", ROOT_PARENT_ID, "folder"),
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:01:00Z",
        )
        .expect("create displaced directory");
    database
        .confirm_present(
            &first,
            &file_facts(
                "child",
                "dir",
                "child.txt",
                "2026-01-01T00:02:00Z",
                10,
            ),
            "2026-01-01T00:03:00Z",
        )
        .expect("create child row");
    database
        .confirm_present(
            &first,
            &file_facts(
                "already-tombstoned",
                "dir",
                "old.txt",
                "2026-01-01T00:04:00Z",
                11,
            ),
            "2026-01-01T00:05:00Z",
        )
        .expect("create soon-tombstoned row");
    database
        .confirm_absent(
            &first,
            &identity("already-tombstoned", "dir", "old.txt"),
        )
        .expect("tombstone child before cascade");
    database
        .confirm_present(
            &first,
            &file_facts(
                "outside",
                ROOT_PARENT_ID,
                "outside.txt",
                "2026-01-01T00:06:00Z",
                12,
            ),
            "2026-01-01T00:07:00Z",
        )
        .expect("create outside row");

    database
        .complete_directory_creation(
            &second,
            &identity("dir", ROOT_PARENT_ID, "folder"),
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:09:00Z",
        )
        .expect("create matching directory in second database");

    database
        .complete_directory_displacement_cascade(&first, &identity("dir", ROOT_PARENT_ID, "folder"))
        .expect("cascade directory displacement");

    assert_eq!(
        database
            .lookup_row(&first, "dir")
            .expect("lookup displaced directory")
            .expect("displaced directory remains")
            .deleted_time
            .as_deref(),
        Some("2026-01-01T00:01:00Z")
    );
    assert_eq!(
        database
            .lookup_row(&first, "child")
            .expect("lookup displaced child")
            .expect("displaced child remains")
            .deleted_time
            .as_deref(),
        Some("2026-01-01T00:01:00Z")
    );
    assert_eq!(
        database
            .lookup_row(&first, "already-tombstoned")
            .expect("lookup old tombstone")
            .expect("old tombstone remains")
            .deleted_time
            .as_deref(),
        Some("2026-01-01T00:05:00Z")
    );
    assert_eq!(
        database
            .lookup_row(&first, "outside")
            .expect("lookup outside row")
            .expect("outside row remains")
            .deleted_time,
        None
    );
    assert_eq!(
        database
            .lookup_row(&second, "dir")
            .expect("lookup second database row")
            .expect("second database row remains")
            .deleted_time,
        None
    );

    database
        .cleanup_old_rows(&first, "2026-01-01T00:04:30Z")
        .expect("cleanup old rows");
    assert!(
        database
            .lookup_row(&first, "dir")
            .expect("lookup cleaned directory")
            .is_none()
    );
    assert!(
        database
            .lookup_row(&first, "child")
            .expect("lookup cleaned child")
            .is_none()
    );
    assert!(
        database
            .lookup_row(&first, "already-tombstoned")
            .expect("lookup retained tombstone")
            .is_some()
    );
}

#[test]
fn open_existing_rejects_schema_drift_instead_of_adapting_the_database() {
    let database = subject();
    let path = temp_snapshot_path("schema-drift");
    let handle = database.create_empty(&path).expect("create snapshot database");
    let prepared_path = database
        .prepare_for_upload(handle)
        .expect("prepare snapshot database for external edit");

    let connection = Connection::open(&prepared_path).expect("open prepared database");
    connection
        .execute("CREATE TABLE extra_table (id TEXT)", [])
        .expect("add schema drift");
    drop(connection);

    let error = database
        .open_existing(&prepared_path)
        .expect_err("schema drift is rejected");
    assert_eq!(error.kind, SnapshotDatabaseErrorKind::SchemaValidation);
}
