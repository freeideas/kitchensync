use rusqlite::{params, Connection};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn temp_root() -> Result<PathBuf, Box<dyn Error>> {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kitchensync-sqlite-snapshot-{}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn assert_no_sidecars(db_path: &Path) -> Result<(), Box<dyn Error>> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let sidecar = db_path.with_file_name(format!(
            "{}{}",
            db_path.file_name().unwrap().to_string_lossy(),
            suffix
        ));
        assert!(
            !sidecar.exists(),
            "sidecar still exists after closed rollback-journal database: {}",
            sidecar.display()
        );
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = temp_root()?;
    let db_path = root.join("snapshot.db");

    {
        let mut conn = Connection::open(&db_path)?;
        let mode: String = conn.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))?;
        assert_eq!(mode.to_ascii_lowercase(), "delete");
        conn.execute_batch(
            "
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT NOT NULL,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            );
            CREATE INDEX snapshot_parent_id ON snapshot(parent_id);
            CREATE INDEX snapshot_last_seen ON snapshot(last_seen);
            CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time);
            ",
        )?;

        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO snapshot VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                "dir00000000",
                "root0000000",
                "dir",
                "2024-01-01_10-00-00_000000Z",
                -1_i64,
                "2024-01-01_10-00-01_000000Z"
            ],
        )?;
        tx.execute(
            "INSERT INTO snapshot VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                "file0000000",
                "dir00000000",
                "file.txt",
                "2024-01-01_10-00-00_000000Z",
                5_i64,
                "2024-01-01_10-00-01_000000Z"
            ],
        )?;
        tx.execute(
            "INSERT INTO snapshot VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                "other000000",
                "root0000000",
                "other.txt",
                "2024-01-01_10-00-00_000000Z",
                3_i64,
                "2024-01-01_10-00-01_000000Z"
            ],
        )?;
        tx.commit()?;

        let changed = conn.execute(
            "
            WITH RECURSIVE subtree(id) AS (
                VALUES(?1)
                UNION ALL
                SELECT s.id FROM snapshot s
                JOIN subtree st ON s.parent_id = st.id
                WHERE s.deleted_time IS NULL
            )
            UPDATE snapshot
            SET deleted_time = ?2
            WHERE deleted_time IS NULL
            AND id IN (SELECT id FROM subtree);
            ",
            params!["dir00000000", "2024-01-01_10-00-01_000000Z"],
        )?;
        assert_eq!(changed, 2);

        let deleted: String = conn.query_row(
            "SELECT deleted_time FROM snapshot WHERE id = 'file0000000'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(deleted, "2024-01-01_10-00-01_000000Z");

        let other_deleted: Option<String> = conn.query_row(
            "SELECT deleted_time FROM snapshot WHERE id = 'other000000'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(other_deleted, None);
    }

    assert!(db_path.exists());
    assert_no_sidecars(&db_path)?;
    let header = fs::read(&db_path)?;
    assert_eq!(&header[..16], b"SQLite format 3\0");
    fs::remove_dir_all(root)?;

    println!("checked rusqlite rollback-journal snapshot schema, CTE cascade, and closed-file upload state");
    Ok(())
}
