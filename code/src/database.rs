use rusqlite::{Connection, params};
use std::path::Path;

use crate::timestamp;
use crate::path_hash;

const LOCAL_SCHEMA: &str = r#"
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
    mod_time TEXT,
    byte_size INTEGER,
    del_time TEXT
);

CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_del ON snapshot(del_time);
"#;

const PEER_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshot (
    id BLOB PRIMARY KEY,
    parent_id BLOB NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT,
    byte_size INTEGER,
    del_time TEXT
);

CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_del ON snapshot(del_time);

CREATE TABLE IF NOT EXISTS queue (
    path TEXT PRIMARY KEY,
    enqueued_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_queue_enqueued ON queue(enqueued_at);
"#;

pub struct LocalDatabase {
    conn: Connection,
    log_retention_days: u32,
}

impl LocalDatabase {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| e.to_string())?;
        conn.execute_batch(LOCAL_SCHEMA).map_err(|e| e.to_string())?;

        Ok(Self {
            conn,
            log_retention_days: 32,
        })
    }

    pub fn set_log_retention(&mut self, days: u32) {
        self.log_retention_days = days;
    }

    pub fn get_config(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM config WHERE key = ?",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_config(&mut self, key: &str, value: &str) {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)",
                params![key, value],
            )
            .ok();
    }

    pub fn log(&mut self, level: &str, message: &str) {
        let stamp = timestamp::now();

        self.conn
            .execute(
                "INSERT INTO applog (stamp, level, message) VALUES (?, ?, ?)",
                params![stamp, level, message],
            )
            .ok();

        // Purge old logs
        let cutoff = chrono::Utc::now() - chrono::Duration::days(self.log_retention_days as i64);
        let cutoff_str = timestamp::format_timestamp(&cutoff);
        self.conn
            .execute("DELETE FROM applog WHERE stamp < ?", params![cutoff_str])
            .ok();
    }

    pub fn get_snapshot_entry(&self, path: &str, is_dir: bool) -> Option<SnapshotEntry> {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);

        self.conn
            .query_row(
                "SELECT id, parent_id, basename, mod_time, byte_size, del_time FROM snapshot WHERE id = ?",
                params![&id[..]],
                |row| {
                    Ok(SnapshotEntry {
                        id: row.get::<_, Vec<u8>>(0)?.try_into().unwrap_or([0; 8]),
                        parent_id: row.get::<_, Vec<u8>>(1)?.try_into().unwrap_or([0; 8]),
                        basename: row.get(2)?,
                        mod_time: row.get(3)?,
                        byte_size: row.get(4)?,
                        del_time: row.get(5)?,
                    })
                },
            )
            .ok()
    }

    pub fn upsert_snapshot(&mut self, path: &str, is_dir: bool, mod_time: Option<&str>, byte_size: i64) {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);
        let parent = path_hash::parent_path(&normalized);
        let parent_id = path_hash::hash_path(&parent);
        let basename = path_hash::basename(&normalized);

        self.conn
            .execute(
                "INSERT OR REPLACE INTO snapshot (id, parent_id, basename, mod_time, byte_size, del_time) VALUES (?, ?, ?, ?, ?, NULL)",
                params![&id[..], &parent_id[..], basename, mod_time, byte_size],
            )
            .ok();
    }

    pub fn set_del_time(&mut self, path: &str, is_dir: bool, del_time: &str) {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);

        self.conn
            .execute(
                "UPDATE snapshot SET del_time = ? WHERE id = ?",
                params![del_time, &id[..]],
            )
            .ok();
    }

    pub fn clear_del_time(&mut self, path: &str, is_dir: bool) {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);

        self.conn
            .execute(
                "UPDATE snapshot SET del_time = NULL WHERE id = ?",
                params![&id[..]],
            )
            .ok();
    }

    pub fn get_all_live_entries(&self) -> Vec<SnapshotEntry> {
        let mut stmt = self.conn
            .prepare("SELECT id, parent_id, basename, mod_time, byte_size, del_time FROM snapshot WHERE del_time IS NULL")
            .unwrap();

        stmt.query_map([], |row| {
            Ok(SnapshotEntry {
                id: row.get::<_, Vec<u8>>(0)?.try_into().unwrap_or([0; 8]),
                parent_id: row.get::<_, Vec<u8>>(1)?.try_into().unwrap_or([0; 8]),
                basename: row.get(2)?,
                mod_time: row.get(3)?,
                byte_size: row.get(4)?,
                del_time: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn delete_old_tombstones(&mut self, retention_days: u32) {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = timestamp::format_timestamp(&cutoff);
        self.conn
            .execute(
                "DELETE FROM snapshot WHERE del_time IS NOT NULL AND del_time < ?",
                params![cutoff_str],
            )
            .ok();
    }
}

pub struct PeerDatabase {
    conn: Connection,
    pub queue_max_size: usize,
}

impl PeerDatabase {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| e.to_string())?;
        conn.execute_batch(PEER_SCHEMA).map_err(|e| e.to_string())?;

        Ok(Self {
            conn,
            queue_max_size: 10000,
        })
    }

    pub fn get_config(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM config WHERE key = ?",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_config(&mut self, key: &str, value: &str) {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)",
                params![key, value],
            )
            .ok();
    }

    pub fn enqueue(&mut self, path: &str) {
        let now = timestamp::now();

        // Check if path exists and update
        let exists: bool = self.conn
            .query_row(
                "SELECT 1 FROM queue WHERE path = ?",
                params![path],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            self.conn
                .execute(
                    "UPDATE queue SET enqueued_at = ? WHERE path = ?",
                    params![now, path],
                )
                .ok();
            return;
        }

        // Check queue size
        let count: i64 = self.conn
            .query_row("SELECT COUNT(*) FROM queue", [], |row| row.get(0))
            .unwrap_or(0);

        if count as usize >= self.queue_max_size {
            // Delete oldest entry
            self.conn
                .execute(
                    "DELETE FROM queue WHERE path = (SELECT path FROM queue ORDER BY enqueued_at LIMIT 1)",
                    [],
                )
                .ok();
        }

        // Insert new entry
        self.conn
            .execute(
                "INSERT INTO queue (path, enqueued_at) VALUES (?, ?)",
                params![path, now],
            )
            .ok();
    }

    pub fn dequeue(&mut self) -> Option<String> {
        let path: Option<String> = self.conn
            .query_row(
                "SELECT path FROM queue ORDER BY enqueued_at LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();

        if let Some(ref p) = path {
            self.conn
                .execute("DELETE FROM queue WHERE path = ?", params![p])
                .ok();
        }

        path
    }

    pub fn queue_is_empty(&self) -> bool {
        let count: i64 = self.conn
            .query_row("SELECT COUNT(*) FROM queue", [], |row| row.get(0))
            .unwrap_or(0);
        count == 0
    }

    pub fn get_snapshot_entry(&self, path: &str, is_dir: bool) -> Option<SnapshotEntry> {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);

        self.conn
            .query_row(
                "SELECT id, parent_id, basename, mod_time, byte_size, del_time FROM snapshot WHERE id = ?",
                params![&id[..]],
                |row| {
                    Ok(SnapshotEntry {
                        id: row.get::<_, Vec<u8>>(0)?.try_into().unwrap_or([0; 8]),
                        parent_id: row.get::<_, Vec<u8>>(1)?.try_into().unwrap_or([0; 8]),
                        basename: row.get(2)?,
                        mod_time: row.get(3)?,
                        byte_size: row.get(4)?,
                        del_time: row.get(5)?,
                    })
                },
            )
            .ok()
    }

    pub fn upsert_snapshot(&mut self, path: &str, is_dir: bool, mod_time: Option<&str>, byte_size: i64) {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);
        let parent = path_hash::parent_path(&normalized);
        let parent_id = path_hash::hash_path(&parent);
        let basename = path_hash::basename(&normalized);

        self.conn
            .execute(
                "INSERT OR REPLACE INTO snapshot (id, parent_id, basename, mod_time, byte_size, del_time) VALUES (?, ?, ?, ?, ?, NULL)",
                params![&id[..], &parent_id[..], basename, mod_time, byte_size],
            )
            .ok();
    }

    pub fn set_del_time(&mut self, path: &str, is_dir: bool, del_time: &str) {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);

        self.conn
            .execute(
                "UPDATE snapshot SET del_time = ? WHERE id = ?",
                params![del_time, &id[..]],
            )
            .ok();
    }

    pub fn clear_del_time(&mut self, path: &str, is_dir: bool) {
        let normalized = path_hash::normalize_path(path, is_dir);
        let id = path_hash::hash_path(&normalized);

        self.conn
            .execute(
                "UPDATE snapshot SET del_time = NULL WHERE id = ?",
                params![&id[..]],
            )
            .ok();
    }

    pub fn get_all_live_entries(&self) -> Vec<SnapshotEntry> {
        let mut stmt = self.conn
            .prepare("SELECT id, parent_id, basename, mod_time, byte_size, del_time FROM snapshot WHERE del_time IS NULL")
            .unwrap();

        stmt.query_map([], |row| {
            Ok(SnapshotEntry {
                id: row.get::<_, Vec<u8>>(0)?.try_into().unwrap_or([0; 8]),
                parent_id: row.get::<_, Vec<u8>>(1)?.try_into().unwrap_or([0; 8]),
                basename: row.get(2)?,
                mod_time: row.get(3)?,
                byte_size: row.get(4)?,
                del_time: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn delete_old_tombstones(&mut self, retention_days: u32) {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = timestamp::format_timestamp(&cutoff);
        self.conn
            .execute(
                "DELETE FROM snapshot WHERE del_time IS NOT NULL AND del_time < ?",
                params![cutoff_str],
            )
            .ok();
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    pub id: [u8; 8],
    pub parent_id: [u8; 8],
    pub basename: String,
    pub mod_time: Option<String>,
    pub byte_size: Option<i64>,
    pub del_time: Option<String>,
}

impl SnapshotEntry {
    pub fn is_dir(&self) -> bool {
        self.byte_size == Some(-1)
    }

    pub fn is_deleted(&self) -> bool {
        self.del_time.is_some()
    }
}
