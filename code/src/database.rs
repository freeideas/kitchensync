use rusqlite::Connection;
use std::path::Path;

use crate::timestamp;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS applog (
    log_id INTEGER PRIMARY KEY,
    stamp TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_applog_stamp ON applog(stamp);

CREATE TABLE IF NOT EXISTS snapshot (
    id BLOB PRIMARY KEY,
    parent_id BLOB NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT NOT NULL,
    byte_size INTEGER,
    del_time TEXT
);
CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_del ON snapshot(del_time);
";

pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let conn =
        Connection::open(path).map_err(|e| format!("Cannot open database {}: {}", path.display(), e))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| format!("Database pragma error: {}", e))?;
    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("Schema error: {}", e))?;
    Ok(conn)
}

pub fn get_config(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM config WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .ok()
}

pub fn set_config(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
        [key, value],
    )
    .map_err(|e| format!("Config write error: {}", e))?;
    Ok(())
}

pub fn log(conn: &Connection, level: &str, message: &str, retention_days: u64) {
    let stamp = timestamp::now();
    conn.execute(
        "INSERT INTO applog (stamp, level, message) VALUES (?1, ?2, ?3)",
        [&stamp, level, message],
    )
    .ok();
    // Purge old entries
    if let Some(cutoff) = cutoff_timestamp(retention_days) {
        conn.execute("DELETE FROM applog WHERE stamp < ?1", [&cutoff])
            .ok();
    }
}

pub fn snapshot_upsert(
    conn: &Connection,
    id: &[u8],
    parent_id: &[u8],
    basename: &str,
    mod_time: &str,
    byte_size: Option<i64>,
    del_time: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO snapshot (id, parent_id, basename, mod_time, byte_size, del_time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, parent_id, basename, mod_time, byte_size, del_time],
    )
    .map_err(|e| format!("Snapshot upsert error: {}", e))?;
    Ok(())
}

pub struct SnapshotEntry {
    pub id: Vec<u8>,
    pub basename: String,
    pub mod_time: String,
    pub byte_size: Option<i64>,
    pub del_time: Option<String>,
}

pub fn snapshot_children(conn: &Connection, parent_id: &[u8]) -> Vec<SnapshotEntry> {
    let mut stmt = conn
        .prepare("SELECT id, basename, mod_time, byte_size, del_time FROM snapshot WHERE parent_id = ?1")
        .unwrap();
    stmt.query_map([parent_id], |row| {
        Ok(SnapshotEntry {
            id: row.get(0)?,
            basename: row.get(1)?,
            mod_time: row.get(2)?,
            byte_size: row.get(3)?,
            del_time: row.get(4)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn snapshot_lookup(conn: &Connection, id: &[u8]) -> Option<SnapshotEntry> {
    conn.query_row(
        "SELECT id, basename, mod_time, byte_size, del_time FROM snapshot WHERE id = ?1",
        [id],
        |row| {
            Ok(SnapshotEntry {
                id: row.get(0)?,
                basename: row.get(1)?,
                mod_time: row.get(2)?,
                byte_size: row.get(3)?,
                del_time: row.get(4)?,
            })
        },
    )
    .ok()
}

pub fn purge_tombstones(conn: &Connection, retention_days: u64) {
    if let Some(cutoff) = cutoff_timestamp(retention_days) {
        conn.execute(
            "DELETE FROM snapshot WHERE del_time IS NOT NULL AND del_time < ?1",
            [&cutoff],
        )
        .ok();
    }
}

pub fn snapshot_delete(conn: &Connection, id: &[u8]) -> Result<(), String> {
    conn.execute("DELETE FROM snapshot WHERE id = ?1", [id])
        .map_err(|e| format!("Snapshot delete error: {}", e))?;
    Ok(())
}

fn cutoff_timestamp(days: u64) -> Option<String> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    Some(timestamp::format_timestamp(cutoff))
}
