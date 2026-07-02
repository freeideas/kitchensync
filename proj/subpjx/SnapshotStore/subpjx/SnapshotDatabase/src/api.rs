use std::path::{Path, PathBuf};

pub type SnapshotDatabaseResult<T> = Result<T, SnapshotDatabaseError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDatabaseHandle {
    pub(crate) local_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotRowIdentity {
    pub id: String,
    pub parent_id: String,
    pub basename: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotRowFacts {
    pub identity: SnapshotRowIdentity,
    pub mod_time: String,
    pub byte_size: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotRow {
    pub identity: SnapshotRowIdentity,
    pub mod_time: String,
    pub byte_size: i64,
    pub last_seen: Option<String>,
    pub deleted_time: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDatabaseError {
    pub kind: SnapshotDatabaseErrorKind,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotDatabaseErrorKind {
    SqliteOpen,
    RollbackJournalMode,
    SchemaCreation,
    SchemaValidation,
    InvalidRowIdentity,
    RowLookup,
    RowMutation,
    Cleanup,
    TransactionCompletion,
    ResourceFinalization,
    ConnectionClose,
}

pub trait SnapshotDatabase: Send + Sync {
    /// Creates a new empty local temporary `snapshot.db` at `local_path`,
    /// selects SQLite rollback-journal mode, creates exactly the required
    /// `snapshot` table and indexes, and validates the created schema before
    /// returning an opened handle. The database must contain no extra tables,
    /// views, columns, or alternate table names. SQLite open, journal setup,
    /// schema creation, or schema validation failures are reported as errors.
    fn create_empty(
        &self,
        local_path: &Path,
    ) -> SnapshotDatabaseResult<SnapshotDatabaseHandle>;

    /// Opens an existing downloaded local temporary `snapshot.db`, selects
    /// SQLite rollback-journal mode for that opened database, and validates the
    /// schema before returning an opened handle. A database whose schema is not
    /// exactly one `snapshot` table with the required columns and indexes is
    /// rejected instead of adapted. SQLite open, journal setup, or schema
    /// validation failures are reported as errors.
    fn open_existing(
        &self,
        local_path: &Path,
    ) -> SnapshotDatabaseResult<SnapshotDatabaseHandle>;

    /// Looks up one stored snapshot row by row id in the supplied local
    /// temporary database. The result includes tombstone rows that remain in
    /// the table and returns `Ok(None)` when no row with that id exists. SQLite
    /// read errors are reported as errors, and the lookup never reads from any
    /// peer-side `.kitchensync/snapshot.db` path.
    fn lookup_row(
        &self,
        database: &SnapshotDatabaseHandle,
        id: &str,
    ) -> SnapshotDatabaseResult<Option<SnapshotRow>>;

    /// Lists stored rows whose `parent_id` equals the supplied parent id in
    /// the supplied local temporary database. The result includes tombstone
    /// rows until cleanup removes them and does not include a synthetic row for
    /// the sync root. SQLite read errors are reported as errors, and the lookup
    /// never reads from another peer's database.
    fn list_child_rows(
        &self,
        database: &SnapshotDatabaseHandle,
        parent_id: &str,
    ) -> SnapshotDatabaseResult<Vec<SnapshotRow>>;

    /// Confirms an entry is present by upserting the row identified by
    /// `facts.identity` with the supplied observed `mod_time`, observed
    /// `byte_size`, supplied new `last_seen`, and `deleted_time = NULL`.
    /// Row identity must represent a tracked path below the sync root: the
    /// sync root itself is rejected, and `basename` must be present as the
    /// final path component. SQLite write or transaction failures are reported
    /// as errors, and success means SQLite accepted the mutation.
    fn confirm_present(
        &self,
        database: &SnapshotDatabaseHandle,
        facts: &SnapshotRowFacts,
        last_seen: &str,
    ) -> SnapshotDatabaseResult<()>;

    /// Confirms an entry is absent. If the row exists and is not already a
    /// tombstone, its `deleted_time` is set to that row's current `last_seen`
    /// and `last_seen` is left unchanged. If the row is already a tombstone or
    /// does not exist, the operation succeeds without changing a row. Invalid
    /// row identity data, SQLite write errors, or transaction failures are
    /// reported as errors.
    fn confirm_absent(
        &self,
        database: &SnapshotDatabaseHandle,
        identity: &SnapshotRowIdentity,
    ) -> SnapshotDatabaseResult<()>;

    /// Records an intended destination file copy before the file copy
    /// completes. The destination row is upserted with the winning file
    /// `mod_time`, winning file `byte_size`, and `deleted_time = NULL`. An
    /// existing `last_seen` value is preserved; a newly inserted row receives
    /// `last_seen = NULL`. If the process exits before copy completion, this
    /// pending state remains. Invalid row identity data, SQLite write errors,
    /// or transaction failures are reported as errors.
    fn record_intended_file_copy(
        &self,
        database: &SnapshotDatabaseHandle,
        facts: &SnapshotRowFacts,
    ) -> SnapshotDatabaseResult<()>;

    /// Completes a queued file copy after the caller has successfully copied
    /// the file. The destination row's `last_seen` is set to the supplied new
    /// timestamp; the timestamp is never generated by this subproject. Missing
    /// required rows, invalid row identity data, SQLite write errors, or
    /// transaction failures are reported as errors.
    fn complete_file_copy(
        &self,
        database: &SnapshotDatabaseHandle,
        identity: &SnapshotRowIdentity,
        last_seen: &str,
    ) -> SnapshotDatabaseResult<()>;

    /// Completes a successful directory creation by upserting the destination
    /// directory row with the supplied directory `mod_time`, `byte_size = -1`,
    /// supplied new `last_seen`, and `deleted_time = NULL`. Failed directory
    /// creation is represented by not calling this method. Invalid row
    /// identity data, SQLite write errors, or transaction failures are
    /// reported as errors.
    fn complete_directory_creation(
        &self,
        database: &SnapshotDatabaseHandle,
        identity: &SnapshotRowIdentity,
        mod_time: &str,
        last_seen: &str,
    ) -> SnapshotDatabaseResult<()>;

    /// Completes a successful displacement to `BAK/` by setting the row's
    /// `deleted_time` to that row's previous `last_seen` and leaving
    /// `last_seen` unchanged. Failed displacement is represented by not
    /// calling this method. Missing required rows, invalid row identity data,
    /// SQLite write errors, or transaction failures are reported as errors.
    fn complete_displacement(
        &self,
        database: &SnapshotDatabaseHandle,
        identity: &SnapshotRowIdentity,
    ) -> SnapshotDatabaseResult<()>;

    /// Completes a successful directory displacement cascade. The displaced
    /// directory row's previous `last_seen` is used as the deletion estimate,
    /// and that same value is written as `deleted_time` on every non-tombstone
    /// row reachable from the displaced directory by following `parent_id`
    /// links in this same local database. The cascade includes the displaced
    /// directory row, leaves already tombstoned rows unchanged, leaves rows
    /// outside the subtree unchanged, and never touches another peer's
    /// database. Missing required rows, invalid row identity data, SQLite
    /// write errors, or transaction failures are reported as errors.
    fn complete_directory_displacement_cascade(
        &self,
        database: &SnapshotDatabaseHandle,
        identity: &SnapshotRowIdentity,
    ) -> SnapshotDatabaseResult<()>;

    /// Removes old snapshot rows as opportunistic maintenance. Tombstone rows
    /// with `deleted_time` older than `cutoff` are removed. Obsolete
    /// non-tombstone orphan rows with `last_seen` older than the same cutoff
    /// are removed only when the parent chain needed by a directory
    /// displacement cascade is broken. Rows inside the retention window
    /// remain available as snapshot evidence. If no row matches the cleanup
    /// rules, the operation succeeds without changing the database. SQLite
    /// delete errors or transaction failures are reported as errors.
    fn cleanup_old_rows(
        &self,
        database: &SnapshotDatabaseHandle,
        cutoff: &str,
    ) -> SnapshotDatabaseResult<()>;

    /// Prepares a local temporary `snapshot.db` for upload and consumes the
    /// opened handle. Before returning success, every transaction, statement,
    /// cursor, reader, and SQLite connection owned by this subproject for that
    /// file is finished or closed. After success, transport upload can read
    /// the returned closed local file directly, and the database is usable as
    /// one self-contained SQLite database without WAL, SHM, journal, or other
    /// SQLite sidecar files. Transaction finish, resource finalization, or
    /// connection close failures are reported as errors.
    fn prepare_for_upload(
        &self,
        database: SnapshotDatabaseHandle,
    ) -> SnapshotDatabaseResult<PathBuf>;
}
