use std::any::Any;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone)]
pub struct StagingRecoveryPeerHandle {
    pub identity: String,
    pub winning_url: String,
    pub scheme: StagingRecoveryPeerScheme,
    pub handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingRecoveryPeerScheme {
    File,
    Sftp,
}

#[derive(Clone)]
pub struct SwapRecoveryRequest {
    pub peer: StagingRecoveryPeerHandle,
    pub parent_relative_path: String,
    pub bak_timestamp: String,
}

#[derive(Clone)]
pub struct UserDataSwapRecoveryRequest {
    pub peer: StagingRecoveryPeerHandle,
    pub parent_relative_path: String,
    pub basename: String,
    pub encoded_basename: String,
    pub bak_timestamp: String,
}

#[derive(Clone)]
pub struct BakDisplacementRequest {
    pub peer: StagingRecoveryPeerHandle,
    pub parent_relative_path: String,
    pub basename: String,
    pub bak_timestamp: String,
}

#[derive(Clone)]
pub struct TmpStagingPathRequest {
    pub peer: StagingRecoveryPeerHandle,
    pub parent_relative_path: String,
    pub tmp_timestamp: String,
    pub transfer_uuid: String,
}

#[derive(Clone)]
pub struct StagingCleanupRequest {
    pub peer: StagingRecoveryPeerHandle,
    pub parent_relative_path: String,
    pub current_time: SystemTime,
    pub keep_bak_days: u32,
    pub keep_tmp_days: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapRecoveryResult {
    Recovered,
    FailedListing(StagingRecoveryFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BakDisplacementResult {
    pub peer_identity: String,
    pub original_relative_path: String,
    pub bak_relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmpStagingPathResult {
    pub peer_identity: String,
    pub staging_relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingCleanupResult {
    pub peer_identity: String,
    pub parent_relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingRecoveryFailure {
    pub peer_identity: String,
    pub parent_relative_path: String,
    pub operation: StagingRecoveryOperation,
    pub failed_path: Option<String>,
    pub kind: StagingRecoveryFailureKind,
    pub transport_error: Option<StagingRecoveryTransportErrorCategory>,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingRecoveryOperation {
    SwapRecovery,
    UserDataSwapRecovery,
    BakDisplacement,
    TmpStagingPath,
    Cleanup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingRecoveryFailureKind {
    SwapDirectoryListFailed,
    SwapBasenameDecodeFailed,
    SwapStateCheckFailed,
    SwapRenameFailed,
    SwapDeleteFailed,
    SwapCreateBakDirectoryFailed,
    SwapRemoveDirectoryFailed,
    BakCreateDirectoryFailed,
    BakRenameFailed,
    TmpCreateDirectoryFailed,
    TmpPathNotDirectory,
    CleanupListFailed,
    CleanupTimestampInvalid,
    CleanupRemoveDirectoryFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingRecoveryTransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

pub trait StagingRecovery: Send + Sync {
    /// Recovers user-data SWAP state for one peer and one parent directory
    /// before the caller lists that directory for sync decisions.
    ///
    /// The parent path is relative to the peer sync root; an empty parent path
    /// names the sync root. The BAK timestamp must already be a caller-supplied
    /// `YYYY-MM-DD_HH-mm-ss_ffffffZ` string. The operation checks
    /// `<parent>/.kitchensync/SWAP/` directly even though `.kitchensync/` is
    /// not sync input. A missing SWAP directory is `Recovered` and must not
    /// change user data.
    ///
    /// Each direct SWAP child is one encoded basename for a target under the
    /// same parent. For each child, recovery applies the specified `old`,
    /// `new`, and live-target cases before that child directory is removed:
    /// live target plus `old` archives `old` to nearby BAK; `old` plus `new`
    /// with no live target installs `new` and archives `old`; only `old`
    /// restores `old`; live target plus only `new` deletes `new`; only `new`
    /// installs `new`. Any archive destination is always
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, and the required
    /// BAK parents are created before moving `old`.
    ///
    /// `Recovered` means every direct user-data SWAP child present for this
    /// parent was fully handled and each completed child directory was
    /// removed. `FailedListing` means a filesystem operation, path decoding,
    /// existence check, rename, delete, directory creation, or cleanup step
    /// failed; the caller must treat this peer's live listing for the current
    /// directory as failed and leave this peer's snapshot rows for the current
    /// directory subtree unchanged. Unrecovered SWAP state must remain in
    /// place for a later successful recovery. This method never recovers
    /// `.kitchensync/SWAP/snapshot.db/` and never purges SWAP by age.
    fn recover_swap(&self, request: SwapRecoveryRequest) -> SwapRecoveryResult;

    /// Recovers the one user-data SWAP directory for a target path before a
    /// caller starts replacing that target.
    ///
    /// The parent path is relative to the peer sync root; an empty parent path
    /// names the sync root. The basename is the live entry name for
    /// `<parent>/<basename>`, and `encoded_basename` is the single path
    /// segment naming that target below
    /// `<parent>/.kitchensync/SWAP/`. The BAK timestamp must already be a
    /// caller-supplied `YYYY-MM-DD_HH-mm-ss_ffffffZ` string.
    ///
    /// A missing SWAP directory for the encoded basename succeeds without
    /// changing user data. When that SWAP directory exists, this method
    /// recovers only that encoded child and applies the same `old`, `new`, and
    /// live-target cases as directory-level SWAP recovery: live target plus
    /// `old` archives `old` to nearby BAK; `old` plus `new` with no live
    /// target installs `new` and archives `old`; only `old` restores `old`;
    /// live target plus only `new` deletes `new`; only `new` installs `new`.
    /// Any archive destination is always
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, and the required
    /// BAK parents are created before moving `old`.
    ///
    /// Success means the encoded SWAP child was absent or was fully recovered
    /// and its empty SWAP directory was removed. On failure, the caller must
    /// not start replacement for `<parent>/<basename>`. Unrecovered SWAP state
    /// must remain in place for a later successful recovery. This method must
    /// not recover sibling SWAP children, list live user entries, update
    /// snapshot rows, recover `.kitchensync/SWAP/snapshot.db/`, purge SWAP by
    /// age, choose another timestamp or encoded basename, retry, suppress
    /// writes for dry-run mode, or format output.
    fn recover_user_data_swap(
        &self,
        request: UserDataSwapRecoveryRequest,
    ) -> Result<(), StagingRecoveryFailure>;

    /// Moves one existing user entry from its live path into nearby BAK
    /// storage on one peer.
    ///
    /// The parent path is relative to the peer sync root; an empty parent path
    /// names the sync root. The basename is the single live entry name under
    /// that parent, and the BAK timestamp must already be a caller-supplied
    /// `YYYY-MM-DD_HH-mm-ss_ffffffZ` string. The operation first creates
    /// `<parent>/.kitchensync/BAK/<timestamp>/` and any missing metadata
    /// parents below the same parent directory, then renames
    /// `<parent>/<basename>` to
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
    ///
    /// On success, the original live path is absent and the returned BAK path
    /// names the moved entry. If the displaced entry is a directory, it is
    /// moved as one entry and its subtree is preserved below the BAK
    /// destination. The BAK destination must be under the displaced entry's own
    /// parent directory, not under a root-level aggregate BAK directory unless
    /// that parent is the sync root. Failure to create the BAK directory or
    /// move the entry returns a failure with path context; this method must not
    /// choose another timestamp, delete the original as a fallback, update
    /// snapshot rows, or format output.
    fn displace_to_bak(
        &self,
        request: BakDisplacementRequest,
    ) -> Result<BakDisplacementResult, StagingRecoveryFailure>;

    /// Creates or returns a TMP staging directory for one transfer on one
    /// peer without touching live user paths.
    ///
    /// The parent path is relative to the peer sync root; an empty parent path
    /// names the sync root. The TMP timestamp must already be a caller-supplied
    /// `YYYY-MM-DD_HH-mm-ss_ffffffZ` string, and the transfer UUID is used as
    /// its own path segment. The successful staging path is
    /// `<parent>/.kitchensync/TMP/<timestamp>/<transfer-uuid>/`.
    ///
    /// This operation creates the TMP timestamp directory, missing metadata
    /// parents below the supplied parent, and the transfer-specific directory
    /// when needed. Repeating the same call is successful only when the
    /// transfer-specific TMP path is usable as a directory. A successful result
    /// means the returned path exists for temporary work and no live user path
    /// under `<parent>` was renamed, deleted, overwritten, or replaced. Failure
    /// to create either TMP directory, or finding that the requested TMP path
    /// cannot be used as a directory, returns a failure with peer and path
    /// context; this method must not choose another timestamp or UUID, remove a
    /// conflicting path, fall back to a live path, update snapshot rows, or
    /// format output.
    fn prepare_tmp_staging_path(
        &self,
        request: TmpStagingPathRequest,
    ) -> Result<TmpStagingPathResult, StagingRecoveryFailure>;

    /// Removes expired BAK and TMP timestamp directories for one peer and one
    /// parent directory after the caller has processed that directory level.
    ///
    /// The parent path is relative to the peer sync root; an empty parent path
    /// names the sync root. The operation checks `.kitchensync/` directly as
    /// metadata and lists only `<parent>/.kitchensync/BAK/` and
    /// `<parent>/.kitchensync/TMP/` for cleanup. Missing cleanup roots are not
    /// failures. Cleanup age is determined from each direct timestamp
    /// directory name and the supplied `current_time`, not from filesystem
    /// creation time, modification time, access time, live entries, or
    /// snapshot rows.
    ///
    /// A successful result means every BAK timestamp directory older than
    /// `keep_bak_days` days and every TMP timestamp directory older than
    /// `keep_tmp_days` days was removed, while non-expired BAK and TMP
    /// timestamp directories were left in place. Repeating cleanup with the
    /// same inputs must remain safe after expired directories have already
    /// been removed. Failure to inspect an existing cleanup root, parse a
    /// staging timestamp that must be evaluated, or remove a selected expired
    /// directory returns a failure with peer and path context. This method must
    /// not purge `.kitchensync/SWAP/` by age, delete unexpired directories,
    /// update snapshot rows, retry, choose other retention values, or format
    /// output.
    fn cleanup_staging(
        &self,
        request: StagingCleanupRequest,
    ) -> Result<StagingCleanupResult, StagingRecoveryFailure>;
}
