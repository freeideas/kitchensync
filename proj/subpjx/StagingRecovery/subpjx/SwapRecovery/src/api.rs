use std::any::Any;
use std::sync::Arc;

#[derive(Clone)]
pub struct SwapRecoveryPeer {
    pub identity: String,
    pub scheme: SwapRecoveryPeerScheme,
    pub handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapRecoveryPeerScheme {
    File,
    Sftp,
}

#[derive(Clone)]
pub struct SwapRecoveryRequest {
    pub peer: SwapRecoveryPeer,
    pub parent_path: String,
    pub bak_timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapRecoveryResult {
    Recovered,
    FailedListing(SwapRecoveryFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapRecoveryFailure {
    pub kind: SwapRecoveryFailureKind,
    pub peer_identity: String,
    pub parent_path: String,
    pub failed_path: Option<String>,
    pub transport_error: Option<SwapRecoveryTransportErrorCategory>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapRecoveryFailureKind {
    SwapDirectoryListFailed,
    SwapBasenameDecodeFailed,
    SwapStateCheckFailed,
    SwapRenameFailed,
    SwapDeleteFailed,
    SwapCreateBakDirectoryFailed,
    SwapCleanupFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapRecoveryTransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

pub trait SwapRecovery: Send + Sync {
    /// Recovers user-data SWAP state for one peer and one parent directory
    /// before the caller lists that directory's live entries for sync
    /// decisions.
    ///
    /// The caller supplies the peer, the parent directory, and the timestamp
    /// path segment to use for BAK destinations. The operation checks
    /// `<parent>/.kitchensync/SWAP/` directly even though `.kitchensync/` is
    /// not sync input. If that SWAP directory is absent, the result is
    /// `Recovered` and user data is unchanged.
    ///
    /// When the SWAP directory exists, each direct user-data child is treated
    /// as the encoded basename for the target `<parent>/<basename>`. For that
    /// child, `old` is
    /// `<parent>/.kitchensync/SWAP/<encoded-basename>/old` and `new` is
    /// `<parent>/.kitchensync/SWAP/<encoded-basename>/new`.
    ///
    /// Recovery applies the specified cases for each child: if `old` and the
    /// target both exist, the target is left in place and `old` is moved to
    /// BAK; if `old` and `new` both exist while the target is missing, `new`
    /// is renamed to the target and `old` is moved to BAK; if only `old`
    /// exists, `old` is renamed back to the target; if `new` and the target
    /// both exist while `old` is missing, the target is left in place and
    /// `new` is deleted; if only `new` exists, `new` is renamed to the target.
    ///
    /// Any BAK destination for `old` is always
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>` under the same
    /// parent directory as the target. The needed BAK parent directories must
    /// be created before moving `old`.
    ///
    /// `Recovered` means every direct user-data SWAP child for this peer and
    /// parent was handled and each completed child directory was removed. A
    /// repeated call after successful recovery is idempotent when no new SWAP
    /// state has appeared: the missing SWAP directory again returns
    /// `Recovered` without changing user data.
    ///
    /// `FailedListing` means a filesystem operation, path decoding, listing,
    /// existence check, rename, delete, directory creation, or cleanup step
    /// failed for this peer and parent. The caller must treat the live listing
    /// for this peer and directory as failed and must leave this peer's
    /// snapshot rows for the current directory subtree unchanged.
    ///
    /// On failure, unrecovered SWAP directories are not deleted as cleanup;
    /// they remain for a later successful recovery. This operation does not
    /// list live user entries, choose peers or parent directories, choose the
    /// BAK timestamp, update snapshot rows, create snapshot tombstones,
    /// recover `.kitchensync/SWAP/snapshot.db/`, perform age-based cleanup of
    /// SWAP, BAK, or TMP directories, format output, retry failed operations,
    /// suppress writes for dry-run mode, or choose the transport
    /// implementation.
    fn recover_swap(&self, request: SwapRecoveryRequest) -> SwapRecoveryResult;
}
