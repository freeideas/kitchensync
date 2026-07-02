use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use snapshotstore_snapshotdatabase_snapshotrows::{
    new, SnapshotRowFacts, SnapshotRowIdentity, SnapshotRows,
};

fn subject() -> Arc<dyn SnapshotRows> {
    new()
}

fn connection_with_snapshot_schema() -> Connection {
    let connection = Connection::open_in_memory().expect("open in-memory snapshot database");
    connection
        .execute_batch(
            "CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            );
            CREATE INDEX snapshot_parent_id ON snapshot(parent_id);
            CREATE INDEX snapshot_last_seen ON snapshot(last_seen);
            CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time);",
        )
        .expect("create snapshot schema");
    connection
}

fn identity(id: &str, parent_id: &str, basename: &str) -> SnapshotRowIdentity {
    SnapshotRowIdentity {
        id: id.to_string(),
        parent_id: parent_id.to_string(),
        basename: basename.to_string(),
    }
}

fn facts(
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

#[derive(Debug, PartialEq, Eq)]
struct StoredRow {
    parent_id: String,
    basename: String,
    mod_time: String,
    byte_size: i64,
    last_seen: Option<String>,
    deleted_time: Option<String>,
}

fn stored_row(connection: &Connection, id: &str) -> Option<StoredRow> {
    connection
        .query_row(
            "SELECT parent_id, basename, mod_time, byte_size, last_seen, deleted_time
             FROM snapshot
             WHERE id = ?1",
            params![id],
            |row| {
                Ok(StoredRow {
                    parent_id: row.get(0)?,
                    basename: row.get(1)?,
                    mod_time: row.get(2)?,
                    byte_size: row.get(3)?,
                    last_seen: row.get(4)?,
                    deleted_time: row.get(5)?,
                })
            },
        )
        .optional()
        .expect("read snapshot row")
}

fn insert_row(
    connection: &Connection,
    id: &str,
    parent_id: &str,
    basename: &str,
    mod_time: &str,
    byte_size: i64,
    last_seen: Option<&str>,
    deleted_time: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO snapshot
             (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                parent_id,
                basename,
                mod_time,
                byte_size,
                last_seen,
                deleted_time
            ],
        )
        .expect("insert snapshot row");
}

#[test]
fn confirm_present_records_current_file_and_directory_facts() {
    let rows = subject();
    let mut connection = connection_with_snapshot_schema();

    rows.confirm_present(
        &mut connection,
        &facts(
            "file-id",
            "dir-id",
            "readme.txt",
            "2026-07-02_10-00-00_000001Z",
            42,
        ),
        "2026-07-02_10-00-01_000001Z",
    )
    .expect("confirm file present");
    rows.confirm_present(
        &mut connection,
        &facts(
            "dir-id",
            "root-id",
            "docs",
            "2026-07-02_10-00-02_000001Z",
            -1,
        ),
        "2026-07-02_10-00-03_000001Z",
    )
    .expect("confirm directory present");

    assert_eq!(
        stored_row(&connection, "file-id"),
        Some(StoredRow {
            parent_id: "dir-id".to_string(),
            basename: "readme.txt".to_string(),
            mod_time: "2026-07-02_10-00-00_000001Z".to_string(),
            byte_size: 42,
            last_seen: Some("2026-07-02_10-00-01_000001Z".to_string()),
            deleted_time: None,
        })
    );
    assert_eq!(
        stored_row(&connection, "dir-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "docs".to_string(),
            mod_time: "2026-07-02_10-00-02_000001Z".to_string(),
            byte_size: -1,
            last_seen: Some("2026-07-02_10-00-03_000001Z".to_string()),
            deleted_time: None,
        })
    );

    rows.confirm_present(
        &mut connection,
        &facts(
            "file-id",
            "dir-id",
            "readme.txt",
            "2026-07-02_10-00-04_000001Z",
            84,
        ),
        "2026-07-02_10-00-05_000001Z",
    )
    .expect("refresh file present");

    assert_eq!(
        stored_row(&connection, "file-id"),
        Some(StoredRow {
            parent_id: "dir-id".to_string(),
            basename: "readme.txt".to_string(),
            mod_time: "2026-07-02_10-00-04_000001Z".to_string(),
            byte_size: 84,
            last_seen: Some("2026-07-02_10-00-05_000001Z".to_string()),
            deleted_time: None,
        })
    );
}

#[test]
fn invalid_identity_values_are_rejected_without_creating_rows() {
    let rows = subject();
    let mut connection = connection_with_snapshot_schema();

    assert!(rows
        .confirm_present(
            &mut connection,
            &facts("", "root-id", "missing-id.txt", "mtime", 1),
            "seen",
        )
        .is_err());
    assert!(rows
        .confirm_present(
            &mut connection,
            &facts("empty-basename", "root-id", "", "mtime", 1),
            "seen",
        )
        .is_err());
    assert!(rows
        .confirm_present(
            &mut connection,
            &facts("nested-basename", "root-id", "dir/file.txt", "mtime", 1),
            "seen",
        )
        .is_err());
    assert!(rows
        .confirm_absent(&mut connection, &identity("empty-parent", "", "file.txt"))
        .is_err());

    let row_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM snapshot", [], |row| row.get(0))
        .expect("count snapshot rows");
    assert_eq!(row_count, 0);
}

#[test]
fn confirm_absent_tombstones_live_rows_and_leaves_other_rows_unchanged() {
    let rows = subject();
    let mut connection = connection_with_snapshot_schema();

    insert_row(
        &connection,
        "live-id",
        "root-id",
        "live.txt",
        "live-mtime",
        7,
        Some("live-seen"),
        None,
    );
    insert_row(
        &connection,
        "tombstone-id",
        "root-id",
        "old.txt",
        "old-mtime",
        9,
        Some("old-seen"),
        Some("old-deleted"),
    );

    rows.confirm_absent(&mut connection, &identity("live-id", "root-id", "live.txt"))
        .expect("confirm live row absent");
    rows.confirm_absent(
        &mut connection,
        &identity("tombstone-id", "root-id", "old.txt"),
    )
    .expect("confirm tombstone absent");
    rows.confirm_absent(
        &mut connection,
        &identity("missing-id", "root-id", "missing.txt"),
    )
    .expect("confirm missing row absent");

    assert_eq!(
        stored_row(&connection, "live-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "live.txt".to_string(),
            mod_time: "live-mtime".to_string(),
            byte_size: 7,
            last_seen: Some("live-seen".to_string()),
            deleted_time: Some("live-seen".to_string()),
        })
    );
    assert_eq!(
        stored_row(&connection, "tombstone-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "old.txt".to_string(),
            mod_time: "old-mtime".to_string(),
            byte_size: 9,
            last_seen: Some("old-seen".to_string()),
            deleted_time: Some("old-deleted".to_string()),
        })
    );
    assert_eq!(stored_row(&connection, "missing-id"), None);
}

#[test]
fn intended_and_completed_file_copy_preserve_the_pending_copy_state_until_success() {
    let rows = subject();
    let mut connection = connection_with_snapshot_schema();

    rows.record_intended_file_copy(
        &mut connection,
        &facts(
            "new-copy-id",
            "root-id",
            "new.txt",
            "winner-mtime",
            123,
        ),
    )
    .expect("record new intended file copy");

    assert_eq!(
        stored_row(&connection, "new-copy-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "new.txt".to_string(),
            mod_time: "winner-mtime".to_string(),
            byte_size: 123,
            last_seen: None,
            deleted_time: None,
        })
    );

    insert_row(
        &connection,
        "existing-copy-id",
        "root-id",
        "existing.txt",
        "old-mtime",
        5,
        Some("existing-seen"),
        Some("existing-deleted"),
    );
    rows.record_intended_file_copy(
        &mut connection,
        &facts(
            "existing-copy-id",
            "root-id",
            "existing.txt",
            "new-winner-mtime",
            456,
        ),
    )
    .expect("record existing intended file copy");

    assert_eq!(
        stored_row(&connection, "existing-copy-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "existing.txt".to_string(),
            mod_time: "new-winner-mtime".to_string(),
            byte_size: 456,
            last_seen: Some("existing-seen".to_string()),
            deleted_time: None,
        })
    );

    rows.complete_file_copy(
        &mut connection,
        &identity("new-copy-id", "root-id", "new.txt"),
        "copy-complete-seen",
    )
    .expect("complete file copy");

    assert_eq!(
        stored_row(&connection, "new-copy-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "new.txt".to_string(),
            mod_time: "winner-mtime".to_string(),
            byte_size: 123,
            last_seen: Some("copy-complete-seen".to_string()),
            deleted_time: None,
        })
    );
}

#[test]
fn completed_directory_creation_records_a_live_directory_row() {
    let rows = subject();
    let mut connection = connection_with_snapshot_schema();

    rows.complete_directory_creation(
        &mut connection,
        &identity("created-dir-id", "root-id", "created"),
        "created-dir-mtime",
        "created-dir-seen",
    )
    .expect("complete directory creation");

    assert_eq!(
        stored_row(&connection, "created-dir-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "created".to_string(),
            mod_time: "created-dir-mtime".to_string(),
            byte_size: -1,
            last_seen: Some("created-dir-seen".to_string()),
            deleted_time: None,
        })
    );
}

#[test]
fn completed_displacement_uses_the_existing_last_seen_as_the_deletion_time() {
    let rows = subject();
    let mut connection = connection_with_snapshot_schema();

    insert_row(
        &connection,
        "displaced-id",
        "root-id",
        "displaced.txt",
        "displaced-mtime",
        11,
        Some("displaced-seen"),
        None,
    );

    rows.complete_displacement(
        &mut connection,
        &identity("displaced-id", "root-id", "displaced.txt"),
    )
    .expect("complete displacement");

    assert_eq!(
        stored_row(&connection, "displaced-id"),
        Some(StoredRow {
            parent_id: "root-id".to_string(),
            basename: "displaced.txt".to_string(),
            mod_time: "displaced-mtime".to_string(),
            byte_size: 11,
            last_seen: Some("displaced-seen".to_string()),
            deleted_time: Some("displaced-seen".to_string()),
        })
    );
}

#[test]
fn directory_displacement_cascade_tombstones_only_live_rows_in_that_peer_subtree() {
    let rows = subject();
    let mut first_peer = connection_with_snapshot_schema();
    let second_peer = connection_with_snapshot_schema();

    for connection in [&first_peer, &second_peer] {
        insert_row(
            connection,
            "dir-id",
            "root-id",
            "docs",
            "dir-mtime",
            -1,
            Some("dir-seen"),
            None,
        );
        insert_row(
            connection,
            "child-id",
            "dir-id",
            "child.txt",
            "child-mtime",
            12,
            Some("child-seen"),
            None,
        );
        insert_row(
            connection,
            "grandchild-id",
            "child-id",
            "grandchild.txt",
            "grandchild-mtime",
            13,
            Some("grandchild-seen"),
            None,
        );
        insert_row(
            connection,
            "already-tombstoned-id",
            "dir-id",
            "already.txt",
            "already-mtime",
            14,
            Some("already-seen"),
            Some("already-deleted"),
        );
        insert_row(
            connection,
            "outside-id",
            "root-id",
            "outside.txt",
            "outside-mtime",
            15,
            Some("outside-seen"),
            None,
        );
    }

    rows.complete_directory_displacement_cascade(
        &mut first_peer,
        &identity("dir-id", "root-id", "docs"),
    )
    .expect("complete directory displacement cascade");

    for id in ["dir-id", "child-id", "grandchild-id"] {
        assert_eq!(
            stored_row(&first_peer, id)
                .expect("read first peer cascaded row")
                .deleted_time,
            Some("dir-seen".to_string())
        );
    }
    assert_eq!(
        stored_row(&first_peer, "already-tombstoned-id")
            .expect("read already tombstoned row")
            .deleted_time,
        Some("already-deleted".to_string())
    );
    assert_eq!(
        stored_row(&first_peer, "outside-id")
            .expect("read outside row")
            .deleted_time,
        None
    );

    for id in [
        "dir-id",
        "child-id",
        "grandchild-id",
        "outside-id",
    ] {
        assert_eq!(
            stored_row(&second_peer, id)
                .expect("read second peer row")
                .deleted_time,
            None
        );
    }
    assert_eq!(
        stored_row(&second_peer, "already-tombstoned-id")
            .expect("read second peer tombstone row")
            .deleted_time,
        Some("already-deleted".to_string())
    );
}
