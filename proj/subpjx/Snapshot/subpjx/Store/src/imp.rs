use std::path::Path;
use std::sync::Arc;
use crate::api::*;

impl std::fmt::Debug for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StoreError {{ detail: {:?} }}", self.detail)
    }
}

struct StoreImpl {
    clock: Arc<dyn snapshot_clock::Clock>,
    identity: Arc<dyn snapshot_identity::Identity>,
}

impl StoreImpl {
    fn open(&self, db_path: &Path) -> Result<rusqlite::Connection, StoreError> {
        rusqlite::Connection::open(db_path).map_err(|e| StoreError { detail: e.to_string() })
    }

    fn ids(&self, path: &str) -> (String, String) {
        (self.identity.identity(path), self.identity.parent_identity(path))
    }

    fn basename(path: &str) -> String {
        let s = path.trim_matches('/');
        s.rsplit('/').next().unwrap_or(s).to_string()
    }
}

fn se(e: rusqlite::Error) -> StoreError {
    StoreError { detail: e.to_string() }
}

impl Store for StoreImpl {
    fn initialize(&self, db_path: &Path) -> Result<(), StoreError> {
        let conn = self.open(db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshot (
                id           TEXT PRIMARY KEY,
                parent_id    TEXT,
                basename     TEXT NOT NULL,
                mod_time     TEXT NOT NULL,
                byte_size    INTEGER NOT NULL,
                last_seen    TEXT,
                deleted_time TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_parent_id    ON snapshot (parent_id);
            CREATE INDEX IF NOT EXISTS idx_last_seen    ON snapshot (last_seen);
            CREATE INDEX IF NOT EXISTS idx_deleted_time ON snapshot (deleted_time);",
        )
        .map_err(se)
    }

    fn read_row(&self, db_path: &Path, path: &str) -> Result<Option<SnapshotRow>, StoreError> {
        let conn = self.open(db_path)?;
        let (id, _) = self.ids(path);
        let result = conn.query_row(
            "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
             FROM snapshot WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(SnapshotRow {
                    id:           row.get(0)?,
                    parent_id:    row.get(1)?,
                    basename:     row.get(2)?,
                    mod_time:     row.get(3)?,
                    byte_size:    row.get(4)?,
                    last_seen:    row.get(5)?,
                    deleted_time: row.get(6)?,
                })
            },
        );
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(se(e)),
        }
    }

    fn record_present(
        &self,
        db_path: &Path,
        path: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), StoreError> {
        let conn = self.open(db_path)?;
        let (id, parent_id) = self.ids(path);
        let basename = Self::basename(path);
        let now = self.clock.now();
        conn.execute(
            "INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(id) DO UPDATE SET
                 mod_time     = excluded.mod_time,
                 byte_size    = excluded.byte_size,
                 last_seen    = excluded.last_seen,
                 deleted_time = NULL",
            rusqlite::params![id, parent_id, basename, mod_time, byte_size, now],
        )
        .map_err(se)?;
        Ok(())
    }

    fn record_absent(&self, db_path: &Path, path: &str) -> Result<(), StoreError> {
        let conn = self.open(db_path)?;
        let (id, _) = self.ids(path);
        // Copy last_seen into deleted_time only for live rows; already-tombstoned rows are
        // untouched, making the operation idempotent (017.7).
        conn.execute(
            "UPDATE snapshot SET deleted_time = last_seen
             WHERE id = ?1 AND deleted_time IS NULL",
            rusqlite::params![id],
        )
        .map_err(se)?;
        Ok(())
    }

    fn record_push(
        &self,
        db_path: &Path,
        path: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), StoreError> {
        let conn = self.open(db_path)?;
        let (id, parent_id) = self.ids(path);
        let basename = Self::basename(path);
        // last_seen is intentionally absent from the ON CONFLICT SET clause so an existing
        // value is preserved and a new row starts with NULL (017.11).
        conn.execute(
            "INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)
             ON CONFLICT(id) DO UPDATE SET
                 mod_time     = excluded.mod_time,
                 byte_size    = excluded.byte_size,
                 deleted_time = NULL",
            rusqlite::params![id, parent_id, basename, mod_time, byte_size],
        )
        .map_err(se)?;
        Ok(())
    }

    fn record_copied(&self, db_path: &Path, path: &str) -> Result<(), StoreError> {
        let conn = self.open(db_path)?;
        let (id, _) = self.ids(path);
        let now = self.clock.now();
        conn.execute(
            "UPDATE snapshot SET last_seen = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        )
        .map_err(se)?;
        Ok(())
    }

    fn record_inline_failed(&self, _db_path: &Path, _path: &str) {
        // Leave the existing row unchanged; no error is raised (017.14).
    }

    fn record_displaced(&self, db_path: &Path, path: &str) -> Result<(), StoreError> {
        let mut conn = self.open(db_path)?;
        let (id, _) = self.ids(path);
        // Wrap both steps in a transaction so the displaced-entry update and the
        // descendant cascade are always applied together.
        let tx = conn.transaction().map_err(se)?;
        // Mark the displaced entry itself (017.15).
        tx.execute(
            "UPDATE snapshot SET deleted_time = last_seen WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(se)?;
        // Cascade to all descendants transitively through parent_id links.
        // Rows that already carry a deleted_time are left unchanged (017.18).
        tx.execute(
            "WITH RECURSIVE desc(id) AS (
                 SELECT id FROM snapshot WHERE parent_id = ?1
                 UNION ALL
                 SELECT s.id FROM snapshot s JOIN desc d ON s.parent_id = d.id
             )
             UPDATE snapshot SET deleted_time = last_seen
             WHERE id IN (SELECT id FROM desc) AND deleted_time IS NULL",
            rusqlite::params![id],
        )
        .map_err(se)?;
        tx.commit().map_err(se)
    }

    fn prune(&self, db_path: &Path, keep_del_days: u32) -> Result<(), StoreError> {
        let conn = self.open(db_path)?;
        let cutoff = cutoff_timestamp(keep_del_days);
        // Remove tombstone rows whose deleted_time is older than the window (018.1, 018.2).
        conn.execute(
            "DELETE FROM snapshot WHERE deleted_time IS NOT NULL AND deleted_time < ?1",
            rusqlite::params![cutoff],
        )
        .map_err(se)?;
        // Remove live rows that traversal did not visit within the window (018.3).
        conn.execute(
            "DELETE FROM snapshot
             WHERE deleted_time IS NULL AND last_seen IS NOT NULL AND last_seen < ?1",
            rusqlite::params![cutoff],
        )
        .map_err(se)?;
        Ok(())
    }
}

pub fn new(
    clock: Arc<dyn snapshot_clock::Clock>,
    identity: Arc<dyn snapshot_identity::Identity>,
) -> Arc<dyn Store> {
    Arc::new(StoreImpl { clock, identity })
}

// Compute "now minus keep_del_days days" as a timestamp string in the format
// YYYY-MM-DD_HH-mm-ss_ffffffZ, suitable for lexicographic comparison against
// the same-format timestamps stored in the database.
fn cutoff_timestamp(keep_del_days: u32) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now.saturating_sub(keep_del_days as u64 * 86400);
    let days = (cutoff / 86400) as i64;
    let rem = (cutoff % 86400) as u32;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}_{h:02}-{m:02}-{s:02}_000000Z")
}

// Convert a count of days since the Unix epoch (1970-01-01 = day 0) to a
// proleptic Gregorian calendar date.  Algorithm by Howard Hinnant.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
