use rusqlite::Connection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRowIdentity {
    pub id: String,
    pub parent_id: String,
    pub basename: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRowFacts {
    pub identity: SnapshotRowIdentity,
    pub mod_time: String,
    pub byte_size: i64,
}

pub trait SnapshotRows: Send + Sync {
    /// Confirms that one tracked entry is present in the supplied local peer
    /// snapshot database.
    ///
    /// The identity must represent a child below the sync root: the sync root
    /// itself, an empty id, an empty basename, or identity data whose basename
    /// cannot be the final path component is rejected. On success, the row is
    /// upserted with the supplied `mod_time`, supplied `byte_size`, supplied
    /// new `last_seen`, and `deleted_time = NULL`. File rows use their byte
    /// size in bytes; directory rows use `byte_size = -1`. The timestamp is
    /// supplied by the caller and is not generated here. SQLite write errors,
    /// rejected identity data, and transaction failures are returned to the
    /// caller, and success is reported only after SQLite accepts the write.
    fn confirm_present(
        &self,
        database: &mut Connection,
        facts: &SnapshotRowFacts,
        last_seen: &str,
    ) -> rusqlite::Result<()>;

    /// Confirms that one tracked entry is absent in the supplied local peer
    /// snapshot database.
    ///
    /// The identity must represent a child below the sync root: the sync root
    /// itself, an empty id, an empty basename, or identity data whose basename
    /// cannot be the final path component is rejected. If the row exists and
    /// is not already a tombstone, `deleted_time` is set to that row's
    /// existing `last_seen` and `last_seen` is left unchanged. If the row is
    /// already a tombstone or no row exists, the operation succeeds without
    /// changing a row. SQLite write errors, rejected identity data, and
    /// transaction failures are returned to the caller.
    fn confirm_absent(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
    ) -> rusqlite::Result<()>;

    /// Records that a destination file copy has been chosen but has not yet
    /// completed.
    ///
    /// The identity must represent a child below the sync root: the sync root
    /// itself, an empty id, an empty basename, or identity data whose basename
    /// cannot be the final path component is rejected. On success, the
    /// destination file row is upserted with the winning file `mod_time`, the
    /// winning file `byte_size`, and `deleted_time = NULL`. An existing
    /// `last_seen` value is preserved. A newly inserted row receives
    /// `last_seen = NULL`, leaving a durable pending-copy row if the process
    /// exits before the copy completes. SQLite write errors, rejected identity
    /// data, and transaction failures are returned to the caller.
    fn record_intended_file_copy(
        &self,
        database: &mut Connection,
        facts: &SnapshotRowFacts,
    ) -> rusqlite::Result<()>;

    /// Completes a destination file copy after the caller has successfully
    /// copied the file.
    ///
    /// The identity must represent a child below the sync root: the sync root
    /// itself, an empty id, an empty basename, or identity data whose basename
    /// cannot be the final path component is rejected. The existing
    /// destination file row's `last_seen` is set to the supplied new
    /// timestamp. This operation does not invent timestamps and does not copy
    /// the file. Missing destination rows, SQLite write errors, rejected
    /// identity data, and transaction failures are returned to the caller.
    fn complete_file_copy(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
        last_seen: &str,
    ) -> rusqlite::Result<()>;

    /// Completes a destination directory creation after the caller has
    /// successfully created the directory.
    ///
    /// The identity must represent a child below the sync root: the sync root
    /// itself, an empty id, an empty basename, or identity data whose basename
    /// cannot be the final path component is rejected. On success, the
    /// destination directory row is upserted with the supplied directory
    /// `mod_time`, `byte_size = -1`, supplied new `last_seen`, and
    /// `deleted_time = NULL`. Failed directory creation is represented by not
    /// calling this method. SQLite write errors, rejected identity data, and
    /// transaction failures are returned to the caller.
    fn complete_directory_creation(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
        mod_time: &str,
        last_seen: &str,
    ) -> rusqlite::Result<()>;

    /// Completes a successful displacement of one entry to `BAK/`.
    ///
    /// The identity must represent a child below the sync root: the sync root
    /// itself, an empty id, an empty basename, or identity data whose basename
    /// cannot be the final path component is rejected. On success,
    /// `deleted_time` is set to the row's existing `last_seen`, and
    /// `last_seen` is left unchanged. This operation does not generate a
    /// deletion timestamp and does not move the entry to `BAK/`. Failed
    /// displacement is represented by not calling this method. Missing rows,
    /// SQLite write errors, rejected identity data, and transaction failures
    /// are returned to the caller.
    fn complete_displacement(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
    ) -> rusqlite::Result<()>;

    /// Completes a successful displacement of one directory and its stored
    /// descendant rows.
    ///
    /// The identity must represent a child below the sync root: the sync root
    /// itself, an empty id, an empty basename, or identity data whose basename
    /// cannot be the final path component is rejected. The displaced
    /// directory row's existing `last_seen` is used as the deletion estimate.
    /// That same value is written as `deleted_time` on every non-tombstone row
    /// in the same supplied database that belongs to the displaced subtree.
    /// The cascade includes the displaced directory row, follows `parent_id`
    /// links to descendants, leaves already tombstoned rows unchanged, leaves
    /// rows outside the subtree unchanged, and never touches another peer's
    /// database. Missing displaced directory rows, SQLite write errors,
    /// rejected identity data, and transaction failures are returned to the
    /// caller.
    fn complete_directory_displacement_cascade(
        &self,
        database: &mut Connection,
        identity: &SnapshotRowIdentity,
    ) -> rusqlite::Result<()>;
}
