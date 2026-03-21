use rusqlite::{Connection, params};
use std::path::Path;
use std::collections::HashMap;

use crate::config::{Config, PeerGroup};
use crate::url_normalize::normalize_url;
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

CREATE TABLE IF NOT EXISTS peer (
    peer_id INTEGER PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS peer_url (
    peer_id INTEGER NOT NULL REFERENCES peer(peer_id),
    normalized_url TEXT NOT NULL UNIQUE,
    PRIMARY KEY (peer_id, normalized_url)
);

CREATE TABLE IF NOT EXISTS snapshot (
    id TEXT NOT NULL,
    peer_id INTEGER NOT NULL REFERENCES peer(peer_id),
    parent_id TEXT NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    last_seen TEXT,
    deleted_time TEXT,
    PRIMARY KEY (id, peer_id)
);
CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_last_seen ON snapshot(last_seen);
CREATE INDEX IF NOT EXISTS idx_snapshot_deleted ON snapshot(deleted_time);
"#;

pub struct Database {
    pub conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("cannot open database: {}", e))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("cannot set pragmas: {}", e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("cannot create schema: {}", e))?;
        Ok(Database { conn })
    }

    pub fn get_config(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM config WHERE key = ?1", params![key], |row| {
                row.get(0)
            })
            .ok()
    }

    pub fn set_config(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| format!("cannot set config: {}", e))?;
        Ok(())
    }

    pub fn log(&self, level: &str, message: &str, configured_level: &str) {
        let level_ord = level_to_ord(level);
        let configured_ord = level_to_ord(configured_level);
        if level_ord > configured_ord {
            return;
        }
        let stamp = timestamp::now();
        let _ = self.conn.execute(
            "INSERT INTO applog (stamp, level, message) VALUES (?1, ?2, ?3)",
            params![stamp, level, message],
        );
        // REQ_LOG_007: info and error messages also printed to stdout
        if level == "info" || level == "error" {
            println!("{}", message);
        }
    }

    pub fn purge_old_logs(&self, retention_days: u32) -> Result<(), String> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = crate::timestamp::format_micros(cutoff.timestamp_micros());
        self.conn
            .execute("DELETE FROM applog WHERE stamp < ?1", params![cutoff_str])
            .map_err(|e| format!("cannot purge logs: {}", e))?;
        Ok(())
    }

    /// Reconcile peer identity: two-pass algorithm.
    /// Returns mapping of (group_index, peer_index) -> peer_id.
    pub fn reconcile_peers(&self, config: &Config) -> Result<HashMap<(usize, usize), i64>, String> {
        let mut config_peer_to_db_id: HashMap<(usize, usize), i64> = HashMap::new();
        let mut db_id_to_config_peer: HashMap<i64, (usize, usize)> = HashMap::new();

        // Pass 1: Recognize
        for (gi, group) in config.peer_groups.iter().enumerate() {
            for (pi, peer) in group.peers.iter().enumerate() {
                let mut matched_ids: Vec<i64> = Vec::new();
                for url_entry in &peer.urls {
                    let norm = normalize_url(url_entry.url_str())
                        .map_err(|e| format!("bad URL '{}': {}", url_entry.url_str(), e))?;
                    if let Ok(peer_id) = self.conn.query_row(
                        "SELECT peer_id FROM peer_url WHERE normalized_url = ?1",
                        params![norm],
                        |row| row.get::<_, i64>(0),
                    ) {
                        if !matched_ids.contains(&peer_id) {
                            matched_ids.push(peer_id);
                        }
                    }
                }

                if matched_ids.is_empty() {
                    // New peer — will be created in pass 2
                    continue;
                }

                // Use lowest peer_id
                matched_ids.sort();
                let surviving_id = matched_ids[0];

                // Check for ambiguity
                if let Some(existing) = db_id_to_config_peer.get(&surviving_id) {
                    return Err(format!(
                        "ambiguous peer identity: peers '{}' (group '{}') and '{}' (group '{}') resolve to the same peer_id {}",
                        config.peer_groups[existing.0].peers[existing.1].name,
                        config.peer_groups[existing.0].name,
                        peer.name,
                        group.name,
                        surviving_id
                    ));
                }

                config_peer_to_db_id.insert((gi, pi), surviving_id);
                db_id_to_config_peer.insert(surviving_id, (gi, pi));

                // Mark other matched IDs for merging
                for &old_id in &matched_ids[1..] {
                    if let Some(existing) = db_id_to_config_peer.get(&old_id) {
                        if *existing != (gi, pi) {
                            return Err(format!(
                                "ambiguous peer identity during merge for peer '{}'",
                                peer.name
                            ));
                        }
                    }
                    db_id_to_config_peer.insert(old_id, (gi, pi));
                }
            }
        }

        // Pass 2: Rewrite
        for (gi, group) in config.peer_groups.iter().enumerate() {
            for (pi, _peer) in group.peers.iter().enumerate() {
                if config_peer_to_db_id.contains_key(&(gi, pi)) {
                    continue;
                }
                // Create new peer
                self.conn
                    .execute("INSERT INTO peer DEFAULT VALUES", [])
                    .map_err(|e| format!("cannot create peer: {}", e))?;
                let new_id = self.conn.last_insert_rowid();
                config_peer_to_db_id.insert((gi, pi), new_id);
                db_id_to_config_peer.insert(new_id, (gi, pi));
            }
        }

        // Migrate snapshot rows for merged peers
        let mut merges: Vec<(i64, i64)> = Vec::new(); // (old_id, surviving_id)
        for (gi, group) in config.peer_groups.iter().enumerate() {
            for (pi, peer) in group.peers.iter().enumerate() {
                let surviving_id = config_peer_to_db_id[&(gi, pi)];
                for url_entry in &peer.urls {
                    let norm = normalize_url(url_entry.url_str()).unwrap_or_default();
                    if let Ok(old_id) = self.conn.query_row(
                        "SELECT peer_id FROM peer_url WHERE normalized_url = ?1",
                        params![norm],
                        |row| row.get::<_, i64>(0),
                    ) {
                        if old_id != surviving_id && !merges.iter().any(|(o, s)| *o == old_id && *s == surviving_id) {
                            merges.push((old_id, surviving_id));
                        }
                    }
                }
            }
        }

        // Rewrite peer_url table — must happen before deleting old peer rows
        // to avoid foreign key constraint violations
        self.conn
            .execute("DELETE FROM peer_url", [])
            .map_err(|e| format!("cannot clear peer_url: {}", e))?;

        for (old_id, surviving_id) in &merges {
            self.conn
                .execute(
                    "UPDATE snapshot SET peer_id = ?1 WHERE peer_id = ?2",
                    params![surviving_id, old_id],
                )
                .map_err(|e| format!("cannot migrate snapshots: {}", e))?;
            self.conn
                .execute("DELETE FROM peer WHERE peer_id = ?1", params![old_id])
                .map_err(|e| format!("cannot delete old peer: {}", e))?;
        }

        for (gi, group) in config.peer_groups.iter().enumerate() {
            for (pi, peer) in group.peers.iter().enumerate() {
                let peer_id = config_peer_to_db_id[&(gi, pi)];

                // Ensure peer row exists
                self.conn
                    .execute(
                        "INSERT OR IGNORE INTO peer (peer_id) VALUES (?1)",
                        params![peer_id],
                    )
                    .map_err(|e| format!("cannot ensure peer: {}", e))?;

                for url_entry in &peer.urls {
                    let norm = normalize_url(url_entry.url_str())
                        .map_err(|e| format!("bad URL: {}", e))?;
                    self.conn
                        .execute(
                            "INSERT INTO peer_url (peer_id, normalized_url) VALUES (?1, ?2)",
                            params![peer_id, norm],
                        )
                        .map_err(|e| format!("cannot insert peer_url: {}", e))?;
                }
            }
        }

        Ok(config_peer_to_db_id)
    }

    /// Look up which peer_id a normalized URL belongs to.
    pub fn lookup_peer_by_url(&self, norm_url: &str) -> Option<i64> {
        self.conn
            .query_row(
                "SELECT peer_id FROM peer_url WHERE normalized_url = ?1",
                params![norm_url],
                |row| row.get(0),
            )
            .ok()
    }

    /// Check if any peer in the given list has snapshot data.
    pub fn any_peer_has_snapshots(&self, peer_ids: &[i64]) -> bool {
        for &pid in peer_ids {
            let count: i64 = self.conn
                .query_row(
                    "SELECT COUNT(*) FROM snapshot WHERE peer_id = ?1",
                    params![pid],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            if count > 0 {
                return true;
            }
        }
        false
    }

    /// Purge expired tombstones and stale rows.
    pub fn purge_tombstones(&self, retention_days: u32) -> Result<(), String> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = crate::timestamp::format_micros(cutoff.timestamp_micros());

        // Purge tombstones older than retention
        self.conn
            .execute(
                "DELETE FROM snapshot WHERE deleted_time IS NOT NULL AND deleted_time < ?1",
                params![cutoff_str],
            )
            .map_err(|e| format!("cannot purge tombstones: {}", e))?;

        // Purge stale rows with NULL deleted_time and old/NULL last_seen
        self.conn
            .execute(
                "DELETE FROM snapshot WHERE deleted_time IS NULL AND (last_seen IS NULL OR last_seen < ?1)",
                params![cutoff_str],
            )
            .map_err(|e| format!("cannot purge stale rows: {}", e))?;

        Ok(())
    }

    /// Upsert a snapshot row.
    pub fn upsert_snapshot(
        &self,
        id: &str,
        peer_id: i64,
        parent_id: &str,
        basename: &str,
        mod_time: &str,
        byte_size: i64,
        last_seen: Option<&str>,
        deleted_time: Option<&str>,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO snapshot (id, peer_id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, peer_id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time],
            )
            .map_err(|e| format!("cannot upsert snapshot: {}", e))?;
        Ok(())
    }

    /// Get snapshot row for a given (id, peer_id).
    pub fn get_snapshot(&self, id: &str, peer_id: i64) -> Option<SnapshotRow> {
        self.conn
            .query_row(
                "SELECT id, peer_id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time \
                 FROM snapshot WHERE id = ?1 AND peer_id = ?2",
                params![id, peer_id],
                |row| {
                    Ok(SnapshotRow {
                        id: row.get(0)?,
                        peer_id: row.get(1)?,
                        parent_id: row.get(2)?,
                        basename: row.get(3)?,
                        mod_time: row.get(4)?,
                        byte_size: row.get(5)?,
                        last_seen: row.get(6)?,
                        deleted_time: row.get(7)?,
                    })
                },
            )
            .ok()
    }

    /// Update last_seen on a snapshot row (after copy completes).
    pub fn update_last_seen(&self, id: &str, peer_id: i64, last_seen: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE snapshot SET last_seen = ?1 WHERE id = ?2 AND peer_id = ?3",
                params![last_seen, id, peer_id],
            )
            .map_err(|e| format!("cannot update last_seen: {}", e))?;
        Ok(())
    }

    /// Mark a snapshot row as deleted (set deleted_time to last_seen).
    pub fn mark_deleted(&self, id: &str, peer_id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE snapshot SET deleted_time = last_seen WHERE id = ?1 AND peer_id = ?2 AND deleted_time IS NULL",
                params![id, peer_id],
            )
            .map_err(|e| format!("cannot mark deleted: {}", e))?;
        Ok(())
    }

    /// Cascade deletion to subtree (for directory displacement).
    pub fn cascade_delete(&self, displaced_id: &str, peer_id: i64, deleted_time: &str) -> Result<(), String> {
        self.conn
            .execute_batch(&format!(
                "WITH RECURSIVE subtree(id) AS (
                    VALUES('{}')
                    UNION ALL
                    SELECT s.id FROM snapshot s
                    JOIN subtree st ON s.parent_id = st.id
                    WHERE s.peer_id = {} AND s.deleted_time IS NULL
                )
                UPDATE snapshot
                SET deleted_time = '{}'
                WHERE peer_id = {} AND deleted_time IS NULL
                AND id IN (SELECT id FROM subtree);",
                displaced_id, peer_id, deleted_time, peer_id
            ))
            .map_err(|e| format!("cannot cascade delete: {}", e))?;
        Ok(())
    }
}

fn level_to_ord(level: &str) -> u8 {
    match level {
        "error" => 0,
        "info" => 1,
        "debug" => 2,
        "trace" => 3,
        _ => 1,
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub id: String,
    pub peer_id: i64,
    pub parent_id: String,
    pub basename: String,
    pub mod_time: String,
    pub byte_size: i64,
    pub last_seen: Option<String>,
    pub deleted_time: Option<String>,
}
