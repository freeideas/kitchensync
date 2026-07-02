use rusqlite::Connection;

pub trait SnapshotCleanup: Send + Sync {
    /// Removes old tombstones and obsolete orphan rows from one open local
    /// snapshot database.
    ///
    /// The database must already be an open, schema-validated local
    /// `snapshot.db` for one peer. This operation mutates only that supplied
    /// database, uses only rows already stored in its `snapshot` table, and
    /// never inspects the live filesystem or any other peer snapshot. The
    /// cutoff is supplied by the caller from `--keep-del-days`; timestamp
    /// strings are treated as sortable snapshot timestamps and are not parsed,
    /// generated, or reformatted here.
    ///
    /// A successful pass leaves no row matching either cleanup rule for the
    /// supplied cutoff. It removes tombstones only when `deleted_time IS NOT
    /// NULL` and `deleted_time` is older than the cutoff; tombstones with
    /// `deleted_time` equal to or newer than the cutoff remain. It removes
    /// obsolete orphan rows only when `deleted_time IS NULL`, `last_seen` is
    /// older than the same cutoff, and the row cannot be reached by a
    /// directory displacement cascade because the needed parent chain is
    /// broken. A row directly below the sync root is not an orphan merely
    /// because the sync root itself has no row.
    ///
    /// If no rows match the cleanup rules, the operation succeeds without
    /// changing the database. Repeating a successful call with the same
    /// database contents and cutoff is a no-op. SQLite delete errors and
    /// transaction failures are returned to the caller; this operation must not
    /// report success for a cleanup pass whose required database writes were
    /// rejected. Cleanup is maintenance work, so sync correctness must not
    /// depend on this operation completing during the current run.
    fn cleanup_snapshot(
        &self,
        database: &mut Connection,
        cutoff_timestamp: &str,
    ) -> rusqlite::Result<()>;
}
