use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

use crate::timestamp;

const SCHEMA: &str = r#"
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
    id BLOB NOT NULL,
    peer TEXT NOT NULL,
    parent_id BLOB NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    last_seen TEXT,
    deleted_time TEXT,
    PRIMARY KEY (id, peer)
);
CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_last_seen ON snapshot(last_seen);
CREATE INDEX IF NOT EXISTS idx_snapshot_deleted ON snapshot(deleted_time);
"#;

pub struct Database {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub id: Vec<u8>,
    pub peer: String,
    pub parent_id: Vec<u8>,
    pub basename: String,
    pub mod_time: String,
    pub byte_size: i64,
    pub last_seen: Option<String>,
    pub deleted_time: Option<String>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create database directory: {}", e))?;
        }
        let conn = Connection::open(path)
            .map_err(|e| format!("Cannot open database {}: {}", path.display(), e))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("Database pragma error: {}", e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("Database schema error: {}", e))?;
        Ok(Database {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_config(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM config WHERE key = ?1", params![key], |row| {
            row.get(0)
        })
        .optional()
        .unwrap_or(None)
    }

    pub fn set_config(&self, key: &str, value: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )
        .ok();
    }

    pub fn log(&self, level: &str, message: &str, log_retention_days: u64) {
        let conn = self.conn.lock().unwrap();
        let stamp = timestamp::now();
        conn.execute(
            "INSERT INTO applog (stamp, level, message) VALUES (?1, ?2, ?3)",
            params![stamp, level, message],
        )
        .ok();
        // Purge old entries on every insert
        if let Some(cutoff) = timestamp::age_days(&stamp).and_then(|_| {
            let dt = chrono::Utc::now() - chrono::Duration::days(log_retention_days as i64);
            Some(dt.format("%Y%m%dT%H%M%S%.6fZ").to_string())
        }) {
            conn.execute("DELETE FROM applog WHERE stamp < ?1", params![cutoff])
                .ok();
        }
    }

    pub fn get_snapshot(&self, id: &[u8], peer: &str) -> Option<SnapshotRow> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, peer, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot WHERE id = ?1 AND peer = ?2",
            params![id, peer],
            |row| {
                Ok(SnapshotRow {
                    id: row.get(0)?,
                    peer: row.get(1)?,
                    parent_id: row.get(2)?,
                    basename: row.get(3)?,
                    mod_time: row.get(4)?,
                    byte_size: row.get(5)?,
                    last_seen: row.get(6)?,
                    deleted_time: row.get(7)?,
                })
            },
        )
        .optional()
        .unwrap_or(None)
    }

    pub fn upsert_snapshot(
        &self,
        id: &[u8],
        peer: &str,
        parent_id: &[u8],
        basename: &str,
        mod_time: &str,
        byte_size: i64,
        last_seen: Option<&str>,
        deleted_time: Option<&str>,
    ) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO snapshot (id, peer, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id, peer) DO UPDATE SET
               parent_id = ?3, basename = ?4, mod_time = ?5, byte_size = ?6,
               last_seen = COALESCE(?7, last_seen), deleted_time = ?8",
            params![id, peer, parent_id, basename, mod_time, byte_size, last_seen, deleted_time],
        )
        .ok();
    }

    /// Upsert for a push decision: set mod_time/byte_size/deleted_time=NULL, do NOT update last_seen.
    pub fn upsert_snapshot_push(
        &self,
        id: &[u8],
        peer: &str,
        parent_id: &[u8],
        basename: &str,
        mod_time: &str,
        byte_size: i64,
    ) {
        let conn = self.conn.lock().unwrap();
        // If row exists, update mod_time/byte_size/deleted_time, keep last_seen.
        // If row doesn't exist, insert with last_seen=NULL.
        conn.execute(
            "INSERT INTO snapshot (id, peer, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)
             ON CONFLICT(id, peer) DO UPDATE SET
               parent_id = ?3, basename = ?4, mod_time = ?5, byte_size = ?6, deleted_time = NULL",
            params![id, peer, parent_id, basename, mod_time, byte_size],
        )
        .ok();
    }

    /// Set last_seen after a completed copy or directory creation.
    pub fn set_last_seen(&self, id: &[u8], peer: &str, last_seen: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshot SET last_seen = ?3 WHERE id = ?1 AND peer = ?2",
            params![id, peer, last_seen],
        )
        .ok();
    }

    /// Mark entry as deleted on a peer. Sets deleted_time to the row's current last_seen.
    pub fn mark_deleted(&self, id: &[u8], peer: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshot SET deleted_time = last_seen WHERE id = ?1 AND peer = ?2 AND deleted_time IS NULL",
            params![id, peer],
        )
        .ok();
    }

    /// Mark entry as deleted with a specific deleted_time value.
    pub fn mark_deleted_with_time(&self, id: &[u8], peer: &str, deleted_time: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshot SET deleted_time = ?3 WHERE id = ?1 AND peer = ?2 AND deleted_time IS NULL",
            params![id, peer, deleted_time],
        )
        .ok();
    }

    /// Cascade delete marks to descendants of a displaced directory.
    pub fn cascade_delete(&self, displaced_id: &[u8], peer: &str, deleted_time: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("").ok(); // ensure no transaction issues
        conn.execute(
            r#"
            WITH RECURSIVE subtree(id) AS (
                VALUES(?1)
                UNION ALL
                SELECT s.id FROM snapshot s
                JOIN subtree st ON s.parent_id = st.id
                WHERE s.peer = ?2 AND s.deleted_time IS NULL
            )
            UPDATE snapshot
            SET deleted_time = ?3
            WHERE peer = ?2 AND deleted_time IS NULL
            AND id IN (SELECT id FROM subtree)
            "#,
            params![displaced_id, peer, deleted_time],
        )
        .ok();
    }

    /// Purge tombstones and stale rows.
    pub fn purge_tombstones(&self, tombstone_retention_days: u64) {
        let cutoff = {
            let dt = chrono::Utc::now() - chrono::Duration::days(tombstone_retention_days as i64);
            dt.format("%Y%m%dT%H%M%S%.6fZ").to_string()
        };
        let conn = self.conn.lock().unwrap();
        // Purge tombstones (deleted_time IS NOT NULL) older than retention
        conn.execute(
            "DELETE FROM snapshot WHERE deleted_time IS NOT NULL AND deleted_time < ?1",
            params![cutoff],
        )
        .ok();
        // Purge stale rows (deleted_time IS NULL, last_seen old or NULL)
        conn.execute(
            "DELETE FROM snapshot WHERE deleted_time IS NULL AND (last_seen IS NULL OR last_seen < ?1)",
            params![cutoff],
        )
        .ok();
    }

    /// Purge old log entries.
    pub fn purge_logs(&self, log_retention_days: u64) {
        let cutoff = {
            let dt = chrono::Utc::now() - chrono::Duration::days(log_retention_days as i64);
            dt.format("%Y%m%dT%H%M%S%.6fZ").to_string()
        };
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM applog WHERE stamp < ?1", params![cutoff])
            .ok();
    }

    /// Confirm entry present on a peer: update last_seen, clear deleted_time.
    pub fn confirm_present(
        &self,
        id: &[u8],
        peer: &str,
        parent_id: &[u8],
        basename: &str,
        mod_time: &str,
        byte_size: i64,
        sync_stamp: &str,
    ) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO snapshot (id, peer, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
             ON CONFLICT(id, peer) DO UPDATE SET
               parent_id = ?3, basename = ?4, mod_time = ?5, byte_size = ?6,
               last_seen = ?7, deleted_time = NULL",
            params![id, peer, parent_id, basename, mod_time, byte_size, sync_stamp],
        )
        .ok();
    }

    /// Confirm entry absent on a peer: set deleted_time = current last_seen.
    pub fn confirm_absent(&self, id: &[u8], peer: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshot SET deleted_time = last_seen WHERE id = ?1 AND peer = ?2 AND deleted_time IS NULL",
            params![id, peer],
        )
        .ok();
    }
}
