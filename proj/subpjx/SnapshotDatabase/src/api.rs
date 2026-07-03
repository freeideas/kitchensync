use std::path::PathBuf;

use peertransportsurface::ConnectedPeerRoot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotDatabaseRunMode {
    Normal,
    DryRun,
}

#[derive(Clone)]
pub struct SnapshotDatabasePrepareRequest {
    pub peer_index: usize,
    pub peer: ConnectedPeerRoot,
    pub local_snapshot_path: PathBuf,
    pub mode: SnapshotDatabaseRunMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotDatabasePrepareResult {
    Prepared(SnapshotDatabasePreparedPeer),
    Excluded(SnapshotDatabaseDiagnostic),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabasePreparedPeer {
    pub peer_index: usize,
    pub local_snapshot_path: PathBuf,
    pub had_snapshot_history: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabasePeerDatabase {
    pub peer_index: usize,
    pub local_snapshot_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseEntryIdentity {
    pub id: String,
    pub parent_id: String,
    pub basename: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseRow {
    pub id: String,
    pub parent_id: Option<String>,
    pub basename: String,
    pub mod_time: String,
    pub byte_size: i64,
    pub last_seen: Option<String>,
    pub deleted_time: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseListedFileRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry: SnapshotDatabaseEntryIdentity,
    pub mod_time: String,
    pub byte_size: i64,
    pub last_seen: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseListedDirectoryRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry: SnapshotDatabaseEntryIdentity,
    pub mod_time: String,
    pub last_seen: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseConfirmedFileRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry: SnapshotDatabaseEntryIdentity,
    pub mod_time: String,
    pub byte_size: i64,
    pub last_seen: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseIntendedCopyRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry: SnapshotDatabaseEntryIdentity,
    pub mod_time: String,
    pub byte_size: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseCompletedCopyRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry_id: String,
    pub last_seen: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseCreatedDirectoryRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry: SnapshotDatabaseEntryIdentity,
    pub mod_time: String,
    pub last_seen: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseConfirmedAbsenceRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseDisplacementRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub entry_id: String,
    pub is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseCleanupRequest {
    pub database: SnapshotDatabasePeerDatabase,
    pub older_than_timestamp: String,
    pub obsolete_untombstoned_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseCleanupResult {
    pub removed_tombstone_rows: usize,
    pub removed_stale_rows: usize,
}

#[derive(Clone)]
pub struct SnapshotDatabaseUploadRequest {
    pub peer_index: usize,
    pub peer: ConnectedPeerRoot,
    pub local_snapshot_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotDatabaseUploadResult {
    Uploaded,
    Failed(SnapshotDatabaseDiagnostic),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseDiagnostic {
    pub level: SnapshotDatabaseDiagnosticLevel,
    pub peer_index: usize,
    pub kind: SnapshotDatabaseDiagnosticKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotDatabaseDiagnosticLevel {
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotDatabaseDiagnosticKind {
    SnapshotStartupFailed,
    SnapshotUploadFailedBeforeSwapOld,
    SnapshotUploadFailedAfterSwapOld,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotDatabaseError {
    pub kind: SnapshotDatabaseErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotDatabaseErrorKind {
    LocalDatabaseError,
    LocalFileError,
    PeerTransportError,
}

pub trait SnapshotDatabase: Send + Sync {
    /// Prepares one reachable peer's local working snapshot database for the
    /// run. In normal mode this first recovers incomplete peer-side
    /// `.kitchensync/SWAP/snapshot.db/` state, then downloads the live
    /// `.kitchensync/snapshot.db` into `local_snapshot_path`. Recovery handles
    /// only the snapshot SWAP paths: when `old` and live exist it deletes
    /// `new` if present and then deletes `old`; when `old` and `new` exist
    /// without live it renames `new` to live and deletes `old`; when only
    /// `old` exists it renames `old` to live; when `new` and live exist
    /// without `old` it deletes `new`; and when only `new` exists it renames
    /// `new` to live. In dry-run mode peer-side SWAP recovery is skipped and
    /// the live snapshot is downloaded as-is when present. If the live
    /// snapshot is not found, the peer remains prepared with a new empty local
    /// database and `had_snapshot_history = false`. Any SWAP recovery or
    /// download failure other than a missing live snapshot returns an
    /// error-level startup diagnostic and excludes only this peer from the
    /// reachable set.
    fn prepare_peer_snapshot(
        &self,
        request: SnapshotDatabasePrepareRequest,
    ) -> SnapshotDatabasePrepareResult;

    /// Creates a new SQLite snapshot database in rollback-journal mode at the
    /// supplied path. The created database has exactly one application table
    /// named `snapshot`, no view or alternate snapshot table, columns `id TEXT
    /// PRIMARY KEY`, `parent_id TEXT`, `basename TEXT NOT NULL`, `mod_time
    /// TEXT NOT NULL`, `byte_size INTEGER NOT NULL`, nullable `last_seen`, and
    /// nullable `deleted_time`, plus non-primary indexes on `parent_id`,
    /// `last_seen`, and `deleted_time`.
    fn create_snapshot_database(&self, path: PathBuf) -> Result<(), SnapshotDatabaseError>;

    /// Looks up one snapshot row in one peer's local temporary `snapshot.db`.
    /// The lookup is peer-local: it never consults or modifies another peer's
    /// database, never treats SQLite sidecar files as peer snapshot state, and
    /// returns the stored path identity fields, modification time, byte size,
    /// last-seen timestamp, and deleted-time timestamp needed by
    /// reconciliation, including intended-copy rows whose `last_seen` is
    /// `NULL`.
    fn read_snapshot_row(
        &self,
        database: SnapshotDatabasePeerDatabase,
        entry_id: String,
    ) -> Result<Option<SnapshotDatabaseRow>, SnapshotDatabaseError>;

    /// Records a file confirmed present by a peer listing. The row is written
    /// in only that peer's database with the listed modification time, listed
    /// byte size, the supplied fresh current `last_seen` timestamp, and
    /// `deleted_time = NULL`.
    fn record_listed_file(
        &self,
        request: SnapshotDatabaseListedFileRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Records a directory confirmed present by a peer listing. The row is
    /// written in only that peer's database with the listed modification time,
    /// `byte_size = -1`, the supplied fresh current `last_seen` timestamp, and
    /// `deleted_time = NULL`.
    fn record_listed_directory(
        &self,
        request: SnapshotDatabaseListedDirectoryRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Records that a peer already has the winning file state and therefore
    /// needs no copy. The row is written in only that peer's database with the
    /// winning modification time and byte size, the supplied fresh current
    /// `last_seen` timestamp, and `deleted_time = NULL`.
    fn record_confirmed_file(
        &self,
        request: SnapshotDatabaseConfirmedFileRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Records an intended destination file copy before the copy completes.
    /// The row is written in only that peer's database with the winning
    /// modification time and byte size and with `deleted_time = NULL`. If the
    /// row is new, `last_seen` remains `NULL`; if the row already exists, its
    /// existing `last_seen` value is preserved until a successful copy is
    /// recorded.
    fn record_intended_file_copy(
        &self,
        request: SnapshotDatabaseIntendedCopyRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Records a successfully completed destination file copy. Only the
    /// destination peer row's `last_seen` is set to the supplied fresh current
    /// timestamp. Callers must not call this operation for a copy that did not
    /// complete successfully, so failed copies leave the row's `last_seen`
    /// unchanged.
    fn record_completed_file_copy(
        &self,
        request: SnapshotDatabaseCompletedCopyRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Records a successfully created destination directory. The row is
    /// written in only that peer's database with the supplied modification
    /// time, `byte_size = -1`, `deleted_time = NULL`, and the supplied fresh
    /// current `last_seen` timestamp. Callers must not call this operation for
    /// a failed directory creation, so failed creations leave the existing row
    /// unchanged.
    fn record_created_directory(
        &self,
        request: SnapshotDatabaseCreatedDirectoryRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Records a confirmed absence only when the peer's row is untombstoned.
    /// The operation copies that row's existing `last_seen` into
    /// `deleted_time`, leaves `last_seen` unchanged, and does not generate a
    /// new timestamp. If the row is missing or already tombstoned, the row set
    /// is left unchanged, making repeated confirmed absence idempotent.
    fn record_confirmed_absence(
        &self,
        request: SnapshotDatabaseConfirmedAbsenceRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Records a successful displacement to BAK. The displaced row's
    /// `deleted_time` is set to its existing `last_seen` and no generated
    /// current timestamp is used. When the displaced entry is a directory, the
    /// same peer database tombstones untombstoned descendant rows using the
    /// displaced entry's copied deletion estimate for every affected
    /// descendant. The cascade is peer-local, leaves already tombstoned
    /// descendants unchanged, and leaves rows outside the displaced subtree
    /// unchanged. Callers must not call this operation for a failed
    /// displacement.
    fn record_successful_displacement(
        &self,
        request: SnapshotDatabaseDisplacementRequest,
    ) -> Result<(), SnapshotDatabaseError>;

    /// Runs opportunistic snapshot row cleanup for one peer database. Cleanup
    /// removes tombstone rows whose `deleted_time` is older than the supplied
    /// cutoff timestamp. It removes untombstoned stale rows only when their IDs
    /// are supplied by the caller as already obsolete and their `last_seen` is
    /// older than the cutoff or `NULL`. Correctness must not depend on this
    /// operation finishing in the current run, and callers schedule it so it
    /// does not delay the first directory scan or the first eligible file
    /// copy.
    fn cleanup_snapshot_rows(
        &self,
        request: SnapshotDatabaseCleanupRequest,
    ) -> Result<SnapshotDatabaseCleanupResult, SnapshotDatabaseError>;

    /// Uploads a completed local temporary snapshot database to the peer's
    /// live `.kitchensync/snapshot.db` path. Before uploading, all SQLite work
    /// against the local file is finished: transactions are committed or
    /// rolled back, statements and readers are finalized, and every connection
    /// to that local file is closed so the upload reads only the single
    /// `snapshot.db` file with no required sidecar. The normal replacement
    /// order is: write and close SWAP `new`, move an existing live snapshot to
    /// SWAP `old`, move `new` into the live path without requiring overwrite
    /// rename behavior, then delete `old`. Callers invoke this only after all
    /// enqueued file copies have completed. A failure before SWAP `old` exists
    /// returns the before-old diagnostic; a failure after SWAP `old` exists
    /// returns the after-old diagnostic and leaves the incomplete SWAP state
    /// for the next normal startup recovery.
    fn upload_snapshot(
        &self,
        request: SnapshotDatabaseUploadRequest,
    ) -> SnapshotDatabaseUploadResult;
}
