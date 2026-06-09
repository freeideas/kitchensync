//! Public interface for the `Store` subproject.
//!
//! Store owns the one SQLite `snapshot` table that records what a single peer
//! held the last time we looked. It works only on the local working copy of a
//! peer's snapshot database -- the temporary file Transfer has already
//! downloaded or created empty -- named here by its filesystem path, and never
//! touches a peer's filesystem directly. Within that working copy Store owns the
//! `snapshot` schema, the per-peer row reads and updates that record what a peer
//! holds, the displacement tombstone cascade, and the opportunistic removal of
//! old rows.
//!
//! Store reuses Identity to turn a tracked entry's relative path into its row
//! `id` and `parent_id`, and reuses Clock for the fresh "now" timestamp it
//! writes into `last_seen`; it computes neither rule itself. It does not decide
//! what to sync, does not move files, and does not move the database file
//! between a peer and the local temp -- Transfer owns that. Every operation acts
//! on exactly one working database, the one named by `db_path`; an operation for
//! one peer never reads or writes another peer's database.

use std::path::Path;

/// A single tracked entry as recorded in a peer's working `snapshot` database.
///
/// The row shape mirrors the `snapshot` table columns. A file and a directory
/// that share a canonical path share the same `id`, so at most one row exists
/// per tracked path (013.20).
pub struct SnapshotRow {
    /// The entry's stable identity from Identity: the base62 xxHash64 of its
    /// canonical relative path. Primary key of the table (013.4, 013.5).
    pub id: String,
    /// The identity of the entry's parent path (013.6).
    pub parent_id: String,
    /// The entry's final path segment (013.7, 013.8).
    pub basename: String,
    /// The entry's modification time, in the Clock timestamp format (013.9,
    /// 013.10).
    pub mod_time: String,
    /// The size in bytes for a regular file, or `-1` for a directory (013.11,
    /// 013.12, 013.13, 013.14).
    pub byte_size: i64,
    /// The fresh timestamp at which traversal last confirmed the entry present,
    /// or `None` when no presence has yet been confirmed -- for example a row
    /// that exists only because of a push decision (013.15).
    pub last_seen: Option<String>,
    /// The tombstone timestamp recording when the entry was observed deleted, or
    /// `None` for a live entry. It is copied from the row's existing `last_seen`,
    /// never freshly generated (013.16).
    pub deleted_time: Option<String>,
}

/// A failure surfaced by Store to its caller.
///
/// Store surfaces the database-open and SQLite errors raised while working on a
/// peer's working database; `detail` carries the underlying message. Store does
/// not decide whether such a failure aborts the run.
pub struct StoreError {
    /// The underlying database-open or SQLite error detail.
    pub detail: String,
}

/// The owner of one peer's working `snapshot` database.
///
/// The `Send + Sync` supertraits let a single instance be shared as
/// `Arc<dyn Store>` across the concurrent components that record peer state.
/// Every method names the working database to act on by `db_path`, the local
/// temporary file Transfer provides; reads and writes touch only that file
/// (017.19).
pub trait Store: Send + Sync {
    /// Initialize the local working database at `db_path` with the `snapshot`
    /// schema.
    ///
    /// After this call the database holds exactly one table, named `snapshot`
    /// (singular, lowercase), and no view (013.1, 013.2, 013.3). The table has
    /// columns `id` TEXT primary key (013.4, 013.5), `parent_id` TEXT (013.6),
    /// `basename` TEXT not null (013.7, 013.8), `mod_time` TEXT not null (013.9,
    /// 013.10), `byte_size` INTEGER not null (013.11, 013.12), `last_seen` TEXT
    /// nullable (013.15), and `deleted_time` TEXT nullable (013.16), plus an
    /// index on `parent_id`, on `last_seen`, and on `deleted_time` (013.17,
    /// 013.18, 013.19). Because `id` is the primary key, writing the same path
    /// again replaces rather than duplicates its row, so at most one row exists
    /// per tracked path (013.20).
    ///
    /// `db_path` is the local working copy Transfer has already downloaded or
    /// created empty; this schema work runs unchanged under `--dry-run`, since
    /// the working copy is local-only state (024.6). Surfaces database-open and
    /// SQLite errors.
    fn initialize(&self, db_path: &Path) -> Result<(), StoreError>;

    /// Read the row this working database records for the tracked entry at
    /// `path`.
    ///
    /// Returns the stored row so another component can compare it against what
    /// it observes, or `None` when no row exists for that path. The entry is
    /// named by its relative `path`; Store derives the row identity from `path`
    /// through Identity. Surfaces SQLite errors.
    fn read_row(&self, db_path: &Path, path: &str) -> Result<Option<SnapshotRow>, StoreError>;

    /// Record that traversal confirmed the entry at `path` present on the peer.
    ///
    /// Upserts the row keyed by the path's identity so it records the entry's
    /// current `mod_time` (017.1) and `byte_size` (017.2), sets `last_seen` to a
    /// fresh timestamp from Clock (017.3), and sets `deleted_time` to NULL
    /// (017.4). When no row exists, the `id` and `parent_id` from Identity and
    /// the `basename` (the path's final segment) create it. `byte_size` is the
    /// file's size in bytes, or `-1` for a directory (013.13, 013.14).
    fn record_present(
        &self,
        db_path: &Path,
        path: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), StoreError>;

    /// Record that traversal confirmed the entry at `path` absent on the peer.
    ///
    /// For a live row (`deleted_time` NULL), set `deleted_time` to the row's
    /// current `last_seen` value (017.5) and leave `last_seen` unchanged (017.6):
    /// Store copies the existing `last_seen` and never mints a new timestamp
    /// here. A row that already carries a `deleted_time` is left unchanged, so
    /// the operation is idempotent (017.7).
    fn record_absent(&self, db_path: &Path, path: &str) -> Result<(), StoreError>;

    /// Record a push decision: the winning state this peer is to receive at
    /// `path`.
    ///
    /// Upserts the destination row with the winning `mod_time` (017.8) and
    /// `byte_size` (017.9) and `deleted_time` NULL (017.10), and does not set
    /// `last_seen`, so when no prior row exists `last_seen` remains NULL
    /// (017.11). Because `last_seen` is never set at push-decision time, a queued
    /// copy that never completes keeps `deleted_time` NULL (017.21) and keeps
    /// `last_seen` unchanged -- NULL for a first-time target -- so the next run
    /// re-enqueues it (017.22); only a completed copy sets `last_seen`. When no
    /// row exists the `id` and `parent_id` from Identity and the `basename`
    /// create it. `byte_size` is the file's size in bytes, or `-1` for a
    /// directory (013.13, 013.14).
    fn record_push(
        &self,
        db_path: &Path,
        path: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), StoreError>;

    /// Record that a queued file copy or an inline directory creation completed
    /// successfully on the peer for the entry at `path`.
    ///
    /// Sets the row's `last_seen` to a fresh timestamp from Clock (017.12,
    /// 017.13). Call this only after the owning component reports the filesystem
    /// operation succeeded.
    fn record_copied(&self, db_path: &Path, path: &str) -> Result<(), StoreError>;

    /// Record that an inline filesystem operation failed on the peer for the
    /// entry at `path`.
    ///
    /// Store leaves the existing row unchanged, records no effect, and does not
    /// retry; it raises no error of its own for this case, so this method returns
    /// nothing (017.14).
    fn record_inline_failed(&self, db_path: &Path, path: &str);

    /// Record a successful displacement of the entry at `path` to BAK/ on the
    /// peer, cascading the tombstone to its descendants.
    ///
    /// Sets the entry's row `deleted_time` to the row's current `last_seen` value
    /// (017.15), then sets `deleted_time` on every descendant row -- the rows
    /// reached transitively through `parent_id` links from the entry's `id`
    /// (017.16) -- touching only those rows and leaving unrelated rows unchanged
    /// (017.17), and never overwriting a descendant that already has
    /// `deleted_time` set (017.18).
    ///
    /// The cascade runs only against this working database (017.19), once per
    /// peer after that peer's displacement succeeds; when several peers lose the
    /// same subtree in one decision, it runs once per peer, each against that
    /// peer's own database (017.20).
    fn record_displaced(&self, db_path: &Path, path: &str) -> Result<(), StoreError>;

    /// Opportunistically remove aged rows from the working database.
    ///
    /// Removes tombstone rows (`deleted_time IS NOT NULL`) whose `deleted_time`
    /// is older than `keep_del_days` days, and keeps those within the window
    /// (018.1, 018.2). Removes a live row (`deleted_time` NULL) that traversal
    /// did not visit when its `last_seen` is older than `keep_del_days` days
    /// (018.3).
    ///
    /// This maintenance is opportunistic: it must not delay the first directory
    /// scan of a run (018.4) or the first eligible file copy (018.5), and the run
    /// exits 0 even when it does not finish removing every eligible row (018.6).
    /// Correctness never depends on it completing.
    fn prune(&self, db_path: &Path, keep_del_days: u32) -> Result<(), StoreError>;
}
