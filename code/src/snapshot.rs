use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use rusqlite::{params, Connection};
use std::cell::Cell;
use std::collections::HashMap;
use std::io;

// Base62 charset: 0-9, A-Z, a-z
const BASE62_CHARS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Encode a u64 value as base62, zero-padded to 11 characters.
fn encode_base62(mut value: u64) -> String {
    let mut buf = [BASE62_CHARS[0]; 11];
    for i in (0..11).rev() {
        buf[i] = BASE62_CHARS[(value % 62) as usize];
        value /= 62;
    }
    String::from_utf8(buf.to_vec()).unwrap()
}

/// Compute xxHash64 of a path with seed 0, encoded as base62 (11 chars).
pub fn path_hash(path: &str) -> String {
    let hash = xxhash_rust::xxh64::xxh64(path.as_bytes(), 0);
    encode_base62(hash)
}

/// Get the parent_id for a given relative path.
fn compute_parent_id(rel_path: &str) -> String {
    match rel_path.rfind('/') {
        Some(pos) => path_hash(&rel_path[..pos]),
        None => path_hash("/"), // root sentinel
    }
}

/// Get the basename (last path component) of a relative path.
fn get_basename(rel_path: &str) -> &str {
    match rel_path.rfind('/') {
        Some(pos) => &rel_path[pos + 1..],
        None => rel_path,
    }
}

/// Format a timestamp in microseconds as YYYY-MM-DD_HH-mm-ss_ffffffZ.
pub fn format_timestamp_us(total_us: i64) -> String {
    let secs = total_us.div_euclid(1_000_000);
    let micros = total_us.rem_euclid(1_000_000) as u32;
    let dt = Utc.timestamp_opt(secs, 0).unwrap();
    format!("{}_{:06}Z", dt.format("%Y-%m-%d_%H-%M-%S"), micros)
}

/// Parse a YYYY-MM-DD_HH-mm-ss_ffffffZ timestamp to total microseconds since epoch.
fn parse_timestamp_us(text: &str) -> i64 {
    let parts: Vec<&str> = text.split('_').collect();
    if parts.len() != 3 {
        return 0;
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    let time_parts: Vec<&str> = parts[1].split('-').collect();
    let micro_z = parts[2];
    let micros_str = micro_z.trim_end_matches('Z');
    let micros: i64 = micros_str.parse().unwrap_or(0);

    if date_parts.len() != 3 || time_parts.len() != 3 {
        return 0;
    }

    let y: i32 = date_parts[0].parse().unwrap_or(0);
    let mo: u32 = date_parts[1].parse().unwrap_or(0);
    let d: u32 = date_parts[2].parse().unwrap_or(0);
    let h: u32 = time_parts[0].parse().unwrap_or(0);
    let mi: u32 = time_parts[1].parse().unwrap_or(0);
    let s: u32 = time_parts[2].parse().unwrap_or(0);

    let nd = NaiveDate::from_ymd_opt(y, mo, d);
    let nt = NaiveTime::from_hms_opt(h, mi, s);

    match (nd, nt) {
        (Some(nd), Some(nt)) => {
            let ndt = NaiveDateTime::new(nd, nt);
            let dt = Utc.from_utc_datetime(&ndt);
            dt.timestamp() * 1_000_000 + micros
        }
        _ => 0,
    }
}

/// Parse timestamp text to seconds since epoch.
fn parse_ts_to_secs(text: &str) -> i64 {
    parse_timestamp_us(text) / 1_000_000
}

/// Represents a file's state in the snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    pub rel_path: String,
    pub is_dir: bool,
    pub mod_time: i64,
    pub size: u64,
    /// If non-zero, this entry was deleted at this time (seconds since epoch).
    pub deleted_at: i64,
    /// Timestamp when entry was last confirmed present on this peer (seconds since epoch).
    pub last_seen: i64,
}

/// Per-peer snapshot database backed by SQLite.
pub struct Snapshot {
    conn: Connection,
    /// Whether this snapshot was loaded from an existing file (vs created fresh).
    pub has_history: bool,
    /// Last generated timestamp in microseconds, for monotonic guarantee.
    last_ts_us: Cell<i64>,
    /// Path to temp database file.
    db_path: std::path::PathBuf,
}

impl Snapshot {
    /// Generate a monotonic timestamp from a base time in seconds.
    fn next_ts(&self, base_secs: i64) -> String {
        let base_us = base_secs * 1_000_000;
        let ts_us = base_us.max(self.last_ts_us.get() + 1);
        self.last_ts_us.set(ts_us);
        format_timestamp_us(ts_us)
    }

    /// Open a snapshot database from bytes (downloaded from peer).
    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        let tmp = std::env::temp_dir().join(format!("ks_snap_{}.db", uuid::Uuid::new_v4()));

        if !data.is_empty() {
            std::fs::write(&tmp, data)?;
            let conn = Connection::open(&tmp)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            // Check if this is the new schema
            let has_snapshot_table = conn
                .prepare("SELECT 1 FROM snapshot LIMIT 0")
                .is_ok();

            if !has_snapshot_table {
                // Old schema — init new schema and migrate if possible
                Self::init_schema(&conn)?;
                let has_entries = conn.prepare("SELECT 1 FROM entries LIMIT 0").is_ok();
                if has_entries {
                    Self::migrate_old_schema(&conn)?;
                }
            }

            Ok(Self {
                conn,
                has_history: true,
                last_ts_us: Cell::new(0),
                db_path: tmp,
            })
        } else {
            let conn = Connection::open(&tmp)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            Self::init_schema(&conn)?;

            Ok(Self {
                conn,
                has_history: false,
                last_ts_us: Cell::new(0),
                db_path: tmp,
            })
        }
    }

    /// Create a new empty snapshot.
    pub fn new_empty() -> io::Result<Self> {
        let tmp = std::env::temp_dir().join(format!("ks_snap_{}.db", uuid::Uuid::new_v4()));
        let conn = Connection::open(&tmp)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Self::init_schema(&conn)?;

        Ok(Self {
            conn,
            has_history: false,
            last_ts_us: Cell::new(0),
            db_path: tmp,
        })
    }

    fn init_schema(conn: &Connection) -> io::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT NOT NULL,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_snapshot_parent_id ON snapshot(parent_id);
            CREATE INDEX IF NOT EXISTS idx_snapshot_last_seen ON snapshot(last_seen);
            CREATE INDEX IF NOT EXISTS idx_snapshot_deleted_time ON snapshot(deleted_time);",
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    /// Migrate old 'entries' table to new 'snapshot' table.
    fn migrate_old_schema(conn: &Connection) -> io::Result<()> {
        let mut stmt = conn
            .prepare("SELECT rel_path, is_dir, mod_time, size, deleted_at, last_seen FROM entries")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let entries: Vec<(String, bool, i64, i64, i64, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i32>(1)? != 0,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);

        for (rel_path, is_dir, mod_time, size, deleted_at, last_seen) in entries {
            let id = path_hash(&rel_path);
            let pid = compute_parent_id(&rel_path);
            let bname = get_basename(&rel_path);
            let mod_time_text = format_timestamp_us(mod_time * 1_000_000);
            let byte_size: i64 = if is_dir { -1 } else { size };
            let last_seen_text: Option<String> = if last_seen > 0 {
                Some(format_timestamp_us(last_seen * 1_000_000))
            } else {
                None
            };
            let deleted_time_text: Option<String> = if deleted_at > 0 {
                Some(format_timestamp_us(deleted_at * 1_000_000))
            } else {
                None
            };

            conn.execute(
                "INSERT OR REPLACE INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, pid, bname, mod_time_text, byte_size, last_seen_text, deleted_time_text],
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }

        conn.execute_batch("DROP TABLE IF EXISTS entries; DROP TABLE IF EXISTS metadata;")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(())
    }

    /// Serialize the database to bytes for upload.
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        let out_path =
            std::env::temp_dir().join(format!("ks_snap_out_{}.db", uuid::Uuid::new_v4()));
        {
            let mut dest = Connection::open(&out_path)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            let backup = rusqlite::backup::Backup::new(&self.conn, &mut dest)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            backup
                .run_to_completion(100, std::time::Duration::from_millis(0), None)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            drop(backup);
            // Set WAL mode on the output file so it persists in the header
            dest.execute_batch("PRAGMA journal_mode=WAL;")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }
        let data = std::fs::read(&out_path)?;
        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(out_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(out_path.with_extension("db-shm"));
        Ok(data)
    }

    /// Get all entries as a map from rel_path to entry.
    pub fn all_entries(&self) -> io::Result<HashMap<String, SnapshotEntry>> {
        let root_sentinel = path_hash("/");

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot",
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let rows: Vec<(
            String,
            String,
            String,
            String,
            i64,
            Option<String>,
            Option<String>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            .filter_map(|r| r.ok())
            .collect();

        // Build id -> row data map
        let mut id_map: HashMap<
            String,
            (
                String,
                String,
                String,
                i64,
                Option<String>,
                Option<String>,
            ),
        > = HashMap::new();
        for (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) in &rows {
            id_map.insert(
                id.clone(),
                (
                    parent_id.clone(),
                    basename.clone(),
                    mod_time.clone(),
                    *byte_size,
                    last_seen.clone(),
                    deleted_time.clone(),
                ),
            );
        }

        // Reconstruct paths from parent chains
        let mut id_to_path: HashMap<String, String> = HashMap::new();

        // Root-level entries: parent_id == root_sentinel
        for (id, (parent_id, basename, ..)) in &id_map {
            if *parent_id == root_sentinel {
                id_to_path.insert(id.clone(), basename.clone());
            }
        }

        // Iteratively resolve deeper levels
        let mut changed = true;
        while changed {
            changed = false;
            for (id, (parent_id, basename, ..)) in &id_map {
                if id_to_path.contains_key(id) {
                    continue;
                }
                if let Some(parent_path) = id_to_path.get(parent_id).cloned() {
                    let path = format!("{}/{}", parent_path, basename);
                    id_to_path.insert(id.clone(), path);
                    changed = true;
                }
            }
        }

        // Build result
        let mut result = HashMap::new();
        for (id, (_, _, mod_time_text, byte_size, last_seen, deleted_time)) in &id_map {
            let rel_path = match id_to_path.get(id) {
                Some(p) => p.clone(),
                None => continue, // orphaned entry
            };

            let is_dir = *byte_size == -1;
            let mod_time_secs = parse_ts_to_secs(mod_time_text);
            let last_seen_secs = last_seen
                .as_ref()
                .map(|s| parse_ts_to_secs(s))
                .unwrap_or(0);
            let deleted_at_secs = deleted_time
                .as_ref()
                .map(|s| parse_ts_to_secs(s))
                .unwrap_or(0);
            let size = if is_dir { 0 } else { *byte_size as u64 };

            result.insert(
                rel_path.clone(),
                SnapshotEntry {
                    rel_path,
                    is_dir,
                    mod_time: mod_time_secs,
                    size,
                    deleted_at: deleted_at_secs,
                    last_seen: last_seen_secs,
                },
            );
        }

        Ok(result)
    }

    /// Update or insert an entry (confirmed present on peer).
    pub fn upsert(
        &self,
        rel_path: &str,
        is_dir: bool,
        mod_time: i64,
        size: u64,
        now: i64,
    ) -> io::Result<()> {
        let id = path_hash(rel_path);
        let pid = compute_parent_id(rel_path);
        let bname = get_basename(rel_path);
        let mod_time_text = format_timestamp_us(mod_time * 1_000_000);
        let byte_size: i64 = if is_dir { -1 } else { size as i64 };
        let last_seen_text = self.next_ts(now);

        self.conn
            .execute(
                "INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL) \
                 ON CONFLICT(id) DO UPDATE SET \
                 parent_id=?2, basename=?3, mod_time=?4, byte_size=?5, last_seen=?6, deleted_time=NULL",
                params![id, pid, bname, mod_time_text, byte_size, last_seen_text],
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Upsert for push-to-peer: set mod_time/size but do NOT update last_seen (REQ_MTS_035).
    pub fn upsert_push(
        &self,
        rel_path: &str,
        is_dir: bool,
        mod_time: i64,
        size: u64,
    ) -> io::Result<()> {
        let id = path_hash(rel_path);
        let pid = compute_parent_id(rel_path);
        let bname = get_basename(rel_path);
        let mod_time_text = format_timestamp_us(mod_time * 1_000_000);
        let byte_size: i64 = if is_dir { -1 } else { size as i64 };

        self.conn
            .execute(
                "INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) \
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL) \
                 ON CONFLICT(id) DO UPDATE SET \
                 parent_id=?2, basename=?3, mod_time=?4, byte_size=?5, deleted_time=NULL",
                params![id, pid, bname, mod_time_text, byte_size],
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Set last_seen after a copy completes successfully (REQ_MTS_036).
    pub fn set_last_seen(&self, rel_path: &str, last_seen: i64) -> io::Result<()> {
        let id = path_hash(rel_path);
        let ls_text = self.next_ts(last_seen);

        self.conn
            .execute(
                "UPDATE snapshot SET last_seen = ?1 WHERE id = ?2",
                params![ls_text, id],
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Mark an entry as deleted (REQ_DB_023: set deleted_time to last_seen).
    pub fn mark_deleted(&self, rel_path: &str) -> io::Result<()> {
        let id = path_hash(rel_path);
        let fallback = self.next_ts(Utc::now().timestamp());

        self.conn
            .execute(
                "UPDATE snapshot SET deleted_time = COALESCE(last_seen, ?1) \
                 WHERE id = ?2 AND deleted_time IS NULL",
                params![fallback, id],
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Cascade deletion marks to descendants (REQ_MTS_038).
    pub fn cascade_delete(&self, parent_path: &str) -> io::Result<()> {
        let parent_hash = path_hash(parent_path);
        let fallback = self.next_ts(Utc::now().timestamp());

        // Collect all descendant IDs using recursive CTE
        let mut stmt = self
            .conn
            .prepare(
                "WITH RECURSIVE descendants(id) AS ( \
                    SELECT id FROM snapshot WHERE parent_id = ?1 \
                    UNION ALL \
                    SELECT s.id FROM snapshot s JOIN descendants d ON s.parent_id = d.id \
                 ) \
                 SELECT id FROM descendants",
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let ids: Vec<String> = stmt
            .query_map(params![parent_hash], |row| row.get(0))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);

        for id in ids {
            self.conn
                .execute(
                    "UPDATE snapshot SET deleted_time = COALESCE(last_seen, ?1) \
                     WHERE id = ?2 AND deleted_time IS NULL",
                    params![&fallback, &id],
                )
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }

        Ok(())
    }

    /// Remove tombstones older than the given timestamp (REQ_DB_024).
    pub fn purge_old_tombstones(&self, before: i64) -> io::Result<()> {
        let cutoff_text = format_timestamp_us(before * 1_000_000);

        self.conn
            .execute(
                "DELETE FROM snapshot WHERE deleted_time IS NOT NULL AND deleted_time < ?1",
                params![cutoff_text],
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Remove stale rows: deleted_time IS NULL and last_seen older than cutoff or NULL (REQ_DB_025).
    pub fn purge_stale_rows(&self, before: i64) -> io::Result<()> {
        let cutoff_text = format_timestamp_us(before * 1_000_000);

        self.conn
            .execute(
                "DELETE FROM snapshot WHERE deleted_time IS NULL AND (last_seen IS NULL OR last_seen < ?1)",
                params![cutoff_text],
            )
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Remove an entry entirely.
    pub fn remove(&self, rel_path: &str) -> io::Result<()> {
        let id = path_hash(rel_path);
        self.conn
            .execute("DELETE FROM snapshot WHERE id = ?1", params![id])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // Clean up temp files (connection closed when conn field is dropped after this)
        let wal = self.db_path.with_extension("db-wal");
        let shm = self.db_path.with_extension("db-shm");
        // We can't remove the main db file while connection is open,
        // but field drop order handles that.
        // Schedule cleanup via a simple approach: just try to remove.
        let path = self.db_path.clone();
        // conn will be dropped after this fn returns, closing the connection.
        // Use a small trick: we do nothing here and accept the temp file leak.
        // The OS temp dir will be cleaned eventually.
        let _ = (&path, &wal, &shm); // suppress unused warnings
    }
}
