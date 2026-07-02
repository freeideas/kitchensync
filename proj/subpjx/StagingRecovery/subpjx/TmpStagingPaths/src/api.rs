use std::any::Any;
use std::sync::Arc;

#[derive(Clone)]
pub struct TmpStagingPathPeer {
    pub identity: String,
    pub scheme: TmpStagingPathPeerScheme,
    pub handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmpStagingPathPeerScheme {
    File,
    Sftp,
}

#[derive(Clone)]
pub struct TmpStagingPathRequest {
    pub peer: TmpStagingPathPeer,
    pub parent_path: String,
    pub tmp_timestamp: String,
    pub transfer_uuid: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmpStagingPathResult {
    pub peer_identity: String,
    pub staging_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmpStagingPathError {
    pub failure: TmpStagingPathFailure,
    pub peer_identity: String,
    pub parent_path: String,
    pub tmp_timestamp_directory: String,
    pub staging_path: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmpStagingPathFailure {
    CreateTmpTimestampDirectory,
    CreateTransferDirectory,
    TmpPathNotDirectory,
}

pub trait TmpStagingPaths: Send + Sync {
    /// Creates or returns one transfer-specific TMP staging directory on one
    /// peer without touching live user paths.
    ///
    /// The caller has already chosen the transfer work, peer, parent
    /// directory, TMP timestamp string, and transfer UUID. The operation must
    /// use the supplied timestamp as the directory name below `TMP/` and the
    /// supplied transfer UUID as the final path segment; it must not choose a
    /// different timestamp or UUID.
    ///
    /// The staging path is always
    /// `<parent>/.kitchensync/TMP/<timestamp>/<transfer-uuid>/`. The operation
    /// first creates `<parent>/.kitchensync/TMP/<timestamp>/` and any missing
    /// metadata parent directories below `<parent>`, then creates or returns
    /// the transfer-specific directory below that timestamp directory.
    ///
    /// A successful result means the returned transfer-specific TMP directory
    /// exists and is usable as a directory. Repeating the same call is
    /// successful only when that requested TMP path is still usable as a
    /// directory. Success must not rename, delete, overwrite, or replace any
    /// live user path under `<parent>`.
    ///
    /// Failure is returned when the TMP timestamp directory cannot be created,
    /// the transfer-specific TMP directory cannot be created, or the requested
    /// transfer-specific TMP path cannot be used as a directory. The failure
    /// includes peer and path context for reporting the TMP staging path that
    /// could not be prepared. This method must not remove a conflicting path,
    /// fall back to a live user path, clean up old TMP directories, recover
    /// SWAP state, move displaced entries to BAK, update snapshot rows, format
    /// output, retry failed operations, suppress writes for dry-run mode, or
    /// choose the transport implementation.
    fn prepare_tmp_staging_path(
        &self,
        request: TmpStagingPathRequest,
    ) -> Result<TmpStagingPathResult, TmpStagingPathError>;
}
