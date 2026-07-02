use rusqlite::{params, Connection, OptionalExtension};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn temp_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kitchensync-sqlite-snapshot-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = temp_root();
    let db_path = root.join("snapshot.db");
    {
        let conn = Connection::open(&db_path)?;
        let mode: String = conn.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))?;
        assert_eq!(mode.to_ascii_lowercase(), "delete");
        conn.execute_batch(
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
        )?;
        conn.execute(
            "INSERT INTO snapshot VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params!["dir", "root", "docs", "2026-07-02_00-00-00_000001Z", -1, "2026-07-02_00-00-00_000002Z"],
        )?;
        conn.execute(
            "INSERT INTO snapshot VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params!["child", "dir", "readme.txt", "2026-07-02_00-00-00_000003Z", 12, "2026-07-02_00-00-00_000004Z"],
        )?;
        conn.execute(
            "INSERT INTO snapshot VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params!["other", "root", "other.txt", "2026-07-02_00-00-00_000005Z", 5, "2026-07-02_00-00-00_000006Z"],
        )?;
        conn.execute(
            "WITH RECURSIVE subtree(id) AS (
                VALUES(?1)
                UNION ALL
                SELECT s.id FROM snapshot s
                JOIN subtree st ON s.parent_id = st.id
                WHERE s.deleted_time IS NULL
            )
            UPDATE snapshot
            SET deleted_time = ?2
            WHERE deleted_time IS NULL
            AND id IN (SELECT id FROM subtree)",
            params!["dir", "2026-07-02_00-00-00_000002Z"],
        )?;
    }

    assert!(db_path.is_file());
    assert!(!db_path.with_extension("db-wal").exists());
    assert!(!db_path.with_extension("db-shm").exists());
    assert!(!db_path.with_extension("db-journal").exists());

    let conn = Connection::open(&db_path)?;
    let child_deleted: Option<String> = conn
        .query_row(
            "SELECT deleted_time FROM snapshot WHERE id = 'child'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    assert_eq!(
        child_deleted.as_deref(),
        Some("2026-07-02_00-00-00_000002Z")
    );
    let other_deleted: Option<String> = conn.query_row(
        "SELECT deleted_time FROM snapshot WHERE id = 'other'",
        [],
        |row| row.get(0),
    )?;
    assert!(other_deleted.is_none());
    drop(conn);
    fs::remove_dir_all(root)?;
    println!("checked rusqlite rollback journal schema, indexes, close, and recursive CTE cascade");
    Ok(())
}

