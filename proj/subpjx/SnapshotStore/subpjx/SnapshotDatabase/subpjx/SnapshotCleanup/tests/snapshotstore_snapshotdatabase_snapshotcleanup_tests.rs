use std::sync::Arc;

use rusqlite::{params, Connection};
use snapshotstore_snapshotdatabase_snapshotcleanup::{new, SnapshotCleanup};

const ROOT_PARENT_ID: &str = "JyBskcNRrBK";

fn subject() -> Arc<dyn SnapshotCleanup> {
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

fn insert_row(
    connection: &Connection,
    id: &str,
    parent_id: &str,
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
                format!("{id}.txt"),
                "2026-01-01T00:00:00Z",
                1_i64,
                last_seen,
                deleted_time
            ],
        )
        .expect("insert snapshot row");
}

fn stored_ids(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare("SELECT id FROM snapshot ORDER BY id")
        .expect("prepare row id query");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query row ids")
        .map(|row| row.expect("read row id"))
        .collect()
}

#[test]
fn cleanup_removes_only_tombstones_older_than_the_cutoff() {
    let cleanup = subject();
    let mut connection = connection_with_snapshot_schema();

    insert_row(
        &connection,
        "old-tombstone",
        ROOT_PARENT_ID,
        Some("2026-01-01T00:00:00Z"),
        Some("2026-01-01T00:00:00Z"),
    );
    insert_row(
        &connection,
        "cutoff-tombstone",
        ROOT_PARENT_ID,
        Some("2026-01-02T00:00:00Z"),
        Some("2026-01-02T00:00:00Z"),
    );
    insert_row(
        &connection,
        "newer-tombstone",
        ROOT_PARENT_ID,
        Some("2026-01-03T00:00:00Z"),
        Some("2026-01-03T00:00:00Z"),
    );
    insert_row(
        &connection,
        "old-live-root-child",
        ROOT_PARENT_ID,
        Some("2026-01-01T00:00:00Z"),
        None,
    );

    cleanup
        .cleanup_snapshot(&mut connection, "2026-01-02T00:00:00Z")
        .expect("cleanup succeeds");

    assert_eq!(
        stored_ids(&connection),
        vec![
            "cutoff-tombstone".to_string(),
            "newer-tombstone".to_string(),
            "old-live-root-child".to_string(),
        ]
    );
}

#[test]
fn cleanup_removes_old_live_rows_only_when_their_parent_chain_is_broken() {
    let cleanup = subject();
    let mut connection = connection_with_snapshot_schema();

    insert_row(
        &connection,
        "reachable-dir",
        ROOT_PARENT_ID,
        Some("2026-01-01T00:00:00Z"),
        None,
    );
    insert_row(
        &connection,
        "reachable-child",
        "reachable-dir",
        Some("2026-01-01T00:00:00Z"),
        None,
    );
    insert_row(
        &connection,
        "old-root-child",
        ROOT_PARENT_ID,
        Some("2026-01-01T00:00:00Z"),
        None,
    );
    insert_row(
        &connection,
        "old-orphan",
        "missing-parent",
        Some("2026-01-01T00:00:00Z"),
        None,
    );
    insert_row(
        &connection,
        "cutoff-orphan",
        "missing-parent",
        Some("2026-01-02T00:00:00Z"),
        None,
    );
    insert_row(
        &connection,
        "newer-orphan",
        "missing-parent",
        Some("2026-01-03T00:00:00Z"),
        None,
    );
    insert_row(
        &connection,
        "tombstone-parent",
        ROOT_PARENT_ID,
        Some("2026-01-03T00:00:00Z"),
        Some("2026-01-03T00:00:00Z"),
    );
    insert_row(
        &connection,
        "child-below-tombstone-parent",
        "tombstone-parent",
        Some("2026-01-01T00:00:00Z"),
        None,
    );

    cleanup
        .cleanup_snapshot(&mut connection, "2026-01-02T00:00:00Z")
        .expect("cleanup succeeds");

    assert_eq!(
        stored_ids(&connection),
        vec![
            "cutoff-orphan".to_string(),
            "newer-orphan".to_string(),
            "old-root-child".to_string(),
            "reachable-child".to_string(),
            "reachable-dir".to_string(),
            "tombstone-parent".to_string(),
        ]
    );
}
