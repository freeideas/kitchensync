use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

pub const SNAPSHOT_ROOT_PARENT_ID: &str = "JyBskcNRrBK";

#[derive(Clone)]
pub struct SnapshotStartupRequest {
    pub run_mode: SnapshotRunMode,
    pub temporary_root: PathBuf,
    pub peers: Vec<SnapshotPeerHandle>,
}

#[derive(Clone)]
pub struct SnapshotPeerHandle {
    pub identity: String,
    pub role: SnapshotPeerRole,
    pub winning_url: String,
    pub scheme: SnapshotPeerScheme,
    pub handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotRunMode {
    Normal,
    DryRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotPeerRole {
    Canon,
    Subordinate,
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotPeerScheme {
    File,
    Sftp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnapshotRunId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotStartupResult {
    pub run_id: SnapshotRunId,
    pub available_peers: Vec<SnapshotStartupPeer>,
    pub unavailable_peers: Vec<UnavailableSnapshotPeer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotStartupPeer {
    pub peer_identity: String,
    pub role: SnapshotPeerRole,
    pub local_snapshot_path: PathBuf,
    pub had_snapshot_history: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnavailableSnapshotPeer {
    pub peer_identity: String,
    pub role: SnapshotPeerRole,
    pub diagnostic: SnapshotStartupDiagnostic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotStartupDiagnostic {
    pub kind: SnapshotStartupFailureKind,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotStartupFailureKind {
    SwapRecoveryFailed,
    SnapshotDownloadFailed,
    LocalDatabaseFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRow {
    pub id: String,
    pub parent_id: String,
    pub basename: String,
    pub mod_time: String,
    pub byte_size: i64,
    pub last_seen: Option<String>,
    pub deleted_time: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotObservedEntry {
    pub peer_identity: String,
    pub relative_path: String,
    pub mod_time: String,
    pub entry_kind: SnapshotEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotEntryKind {
    File { byte_size: u64 },
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIntendedFileCopy {
    pub destination_peer_identity: String,
    pub destination_relative_path: String,
    pub winning_mod_time: String,
    pub winning_byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotUploadResult {
    pub uploaded_peers: Vec<String>,
    pub failed_peers: Vec<SnapshotUploadFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotUploadFailure {
    pub peer_identity: String,
    pub kind: SnapshotUploadFailureKind,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotUploadFailureKind {
    PrepareLocalDatabaseFailed,
    WriteSwapNewFailed,
    MoveLiveToSwapOldFailed,
    MoveNewToLiveFailed,
    RemoveSwapOldFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotStoreError {
    UnknownRun(SnapshotRunId),
    UnknownPeer(String),
    InvalidRelativePath(String),
    InvalidTimestamp(String),
    TimestampUnavailable(String),
    Database(String),
    Transport(String),
    DryRunUploadForbidden,
}

pub trait SnapshotStore: Send + Sync {
    /// Starts snapshot handling for the reachable peers in one run.
    ///
    /// Each available peer gets exactly one local temporary `snapshot.db` under
    /// `temporary_root` for this run, and all later snapshot reads and writes
    /// for that peer use only that local file. In normal mode, peer-side
    /// `.kitchensync/SWAP/snapshot.db/` recovery runs before deciding whether
    /// the peer has live snapshot history. The fixed peer paths are
    /// `.kitchensync/snapshot.db`, `.kitchensync/SWAP/snapshot.db/new`, and
    /// `.kitchensync/SWAP/snapshot.db/old`, with interrupted replacement state
    /// recovered by the rules in the specification. In dry-run mode, this
    /// method must not run peer-side snapshot SWAP recovery or perform any
    /// peer-side mutation.
    ///
    /// If recovery fails for a peer in normal mode, that peer is reported as
    /// unavailable. If downloading live `.kitchensync/snapshot.db` fails with
    /// any error other than not found, that peer is reported as unavailable.
    /// If the live snapshot is not found, the peer remains available with a new
    /// empty local database and `had_snapshot_history = false`. Peer SQLite
    /// sidecar files are never downloaded or treated as snapshot state.
    ///
    /// Local databases created or accepted by this method must use rollback
    /// journal mode and the exact single-table schema required by the
    /// specification. Schema creation or validation failure is reported as a
    /// peer startup failure instead of being silently adapted. The returned
    /// result preserves peer identities and roles so the caller can update the
    /// global reachable set and print diagnostics.
    fn start_run(&self, request: SnapshotStartupRequest) -> SnapshotStartupResult;

    /// Returns the deterministic snapshot row ID for a path below the sync
    /// root.
    ///
    /// The input must be a slash-separated relative path with no leading slash,
    /// no trailing slash, no repeated slash separators, and no `.` or `..`
    /// components. The sync root itself is invalid because it has no snapshot
    /// row. The returned ID is the zero-padded 11-character base62 encoding of
    /// the xxHash64 seed-0 value for the full relative path, using the alphabet
    /// `0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz`.
    fn path_id(&self, relative_path: &str) -> Result<String, SnapshotStoreError>;

    /// Returns the deterministic parent ID for a path below the sync root.
    ///
    /// The input follows the same validation rules as `path_id`. Entries
    /// directly under the sync root return `SNAPSHOT_ROOT_PARENT_ID`; deeper
    /// entries return the path ID of their slash-separated parent directory.
    /// The sync root itself is invalid because no row is stored for it.
    fn parent_path_id(&self, relative_path: &str) -> Result<String, SnapshotStoreError>;

    /// Generates one process-local timestamp string.
    ///
    /// Each successful call returns a UTC microsecond string in exactly
    /// `YYYY-MM-DD_HH-mm-ss_ffffffZ` format. The returned value is strictly
    /// greater than every timestamp previously generated by this process, even
    /// if the system clock repeats or moves backward. Callers that write
    /// `last_seen` must obtain a fresh value for each row and must not reuse
    /// one generated timestamp across multiple snapshot rows.
    fn generate_timestamp(&self) -> Result<String, SnapshotStoreError>;

    /// Looks up one snapshot row by peer identity and relative path.
    ///
    /// The path is relative to the sync root and follows the same validation
    /// rules as `path_id`; the sync root itself is invalid. Lookup reads only
    /// the run's local temporary snapshot database for the named peer. A
    /// missing row returns `Ok(None)`. A tombstone row is returned with its
    /// non-null `deleted_time` intact until cleanup removes it.
    fn lookup_row(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<Option<SnapshotRow>, SnapshotStoreError>;

    /// Confirms that a file or directory is present on one peer.
    ///
    /// This operation upserts the row for `entry.relative_path` in the peer's
    /// local temporary database with the observed modification time, the
    /// observed file size or `-1` for a directory, a newly generated
    /// `last_seen` timestamp, and `deleted_time = NULL`. The sync root itself
    /// is invalid and must not be stored. The returned string is the new
    /// `last_seen` value written for this row.
    fn confirm_present(
        &self,
        run_id: SnapshotRunId,
        entry: SnapshotObservedEntry,
    ) -> Result<String, SnapshotStoreError>;

    /// Confirms that a previously tracked path is absent on one peer.
    ///
    /// If the row exists and is not already a tombstone, this operation sets
    /// `deleted_time` to that row's current `last_seen` and leaves `last_seen`
    /// unchanged. If the row is missing or already has a non-null
    /// `deleted_time`, the operation succeeds without changing the row. It
    /// never generates a new deletion timestamp.
    fn confirm_absent(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<(), SnapshotStoreError>;

    /// Records a decided destination file copy before the copy has completed.
    ///
    /// The destination row is upserted with the winning modification time,
    /// winning byte size, and `deleted_time = NULL`. An existing `last_seen`
    /// value is preserved. If no row exists, the row is inserted with
    /// `last_seen = NULL`, representing a first-time destination whose queued
    /// copy has not completed. This operation does not copy file bytes and
    /// does not mark the destination as confirmed present.
    fn record_intended_file_copy(
        &self,
        run_id: SnapshotRunId,
        copy: SnapshotIntendedFileCopy,
    ) -> Result<(), SnapshotStoreError>;

    /// Marks a queued destination file copy as completed.
    ///
    /// After the caller's queued file copy has succeeded, this operation sets
    /// the destination file row's `last_seen` to one newly generated timestamp
    /// and leaves `deleted_time = NULL`. The returned string is the timestamp
    /// written for this row. Failed copies must not call this method, so an
    /// interrupted queued copy keeps its prior `last_seen` or the `NULL`
    /// `last_seen` inserted by `record_intended_file_copy`.
    fn complete_file_copy(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<String, SnapshotStoreError>;

    /// Marks a destination directory creation as completed.
    ///
    /// After the caller has successfully created the directory on the peer,
    /// this operation upserts the directory row with the supplied modification
    /// time, `byte_size = -1`, `deleted_time = NULL`, and one newly generated
    /// `last_seen` timestamp. The returned string is the timestamp written for
    /// this row. Failed directory creation must not call this method and must
    /// not change the existing snapshot row.
    fn complete_directory_creation(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
        mod_time: String,
    ) -> Result<String, SnapshotStoreError>;

    /// Marks a file displacement to `BAK/` as completed.
    ///
    /// After the peer has successfully moved the file to `BAK/`, this
    /// operation sets the displaced row's `deleted_time` to its previous
    /// `last_seen` and leaves `last_seen` unchanged. It does not generate a
    /// new timestamp. Failed displacement must not call this method and must
    /// not change that peer's existing snapshot row.
    fn complete_file_displacement(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<(), SnapshotStoreError>;

    /// Marks a directory displacement to `BAK/` as completed and cascades it.
    ///
    /// After the peer has successfully moved the directory to `BAK/`, this
    /// operation uses the displaced directory row's previous `last_seen` as the
    /// deletion estimate. It writes that value as `deleted_time` on the
    /// displaced directory row and on every non-tombstone row reachable from
    /// that directory by following `parent_id` links in the same peer's local
    /// database. Already tombstoned rows, rows outside the displaced subtree,
    /// and other peers' databases are not changed. Failed displacement must
    /// not call this method.
    fn complete_directory_displacement(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        relative_path: &str,
    ) -> Result<(), SnapshotStoreError>;

    /// Performs opportunistic cleanup of old snapshot rows for one peer.
    ///
    /// Cleanup removes tombstone rows whose `deleted_time` is older than
    /// `keep_del_days` days and obsolete non-tombstone orphan rows that cannot
    /// be reached by a directory displacement cascade after their `last_seen`
    /// is older than the same age. Cleanup is maintenance work only; sync
    /// decisions must be able to proceed without waiting for this method to
    /// finish in the current run.
    fn cleanup_peer(
        &self,
        run_id: SnapshotRunId,
        peer_identity: &str,
        keep_del_days: u32,
    ) -> Result<(), SnapshotStoreError>;

    /// Uploads every updated local snapshot for this normal run.
    ///
    /// The caller must invoke this only after all enqueued file copies for the
    /// run have completed. This operation uploads every updated local
    /// temporary snapshot for every available peer in the run, including
    /// subordinate peers; the caller does not choose a subset. Before
    /// uploading each peer, this operation commits or rolls back every
    /// transaction it owns for that local `snapshot.db`, finalizes every owned
    /// statement, cursor, and reader, and closes every owned SQLite connection
    /// to that file. Upload reads the closed local file and sends only
    /// `snapshot.db`, never SQLite sidecar files.
    ///
    /// Each peer upload writes `.kitchensync/SWAP/snapshot.db/new`, closes that
    /// peer-side file, moves an existing live `.kitchensync/snapshot.db` to
    /// `.kitchensync/SWAP/snapshot.db/old`, moves `new` into the live path, and
    /// removes `old` after `new` becomes live. If upload fails before `old`
    /// exists, the live snapshot is not removed and leftover `new` is left for
    /// the next normal startup recovery. If upload fails after `old` exists,
    /// the remaining SWAP state is left for the next normal startup recovery.
    /// When overlapping normal runs upload to the same peer, the live peer
    /// snapshot reflects the last completed upload. Dry-run uploads are
    /// rejected with `DryRunUploadForbidden`.
    fn upload_snapshots(
        &self,
        run_id: SnapshotRunId,
    ) -> Result<SnapshotUploadResult, SnapshotStoreError>;
}
