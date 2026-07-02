use std::any::Any;
use std::sync::Arc;

#[derive(Clone)]
pub struct BakDisplacementPeer {
    pub identity: String,
    pub scheme: BakDisplacementPeerScheme,
    pub handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BakDisplacementPeerScheme {
    File,
    Sftp,
}

#[derive(Clone)]
pub struct BakDisplacementRequest {
    pub peer: BakDisplacementPeer,
    pub parent_path: String,
    pub basename: String,
    pub bak_timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BakDisplacementRecord {
    pub peer_identity: String,
    pub original_path: String,
    pub bak_timestamp_directory: String,
    pub bak_destination_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BakDisplacementError {
    pub failure: BakDisplacementFailure,
    pub peer_identity: String,
    pub original_path: String,
    pub bak_timestamp_directory: String,
    pub bak_destination_path: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BakDisplacementFailure {
    CreateBakTimestampDirectory,
    MoveDisplacedEntry,
}

pub trait BakDisplacement: Send + Sync {
    /// Moves one existing user entry from its original location into nearby
    /// BAK storage on the supplied peer.
    ///
    /// The caller has already chosen the entry to displace and supplies the
    /// peer, the parent directory that currently contains the entry, the entry
    /// basename, and the timestamp directory name to use below `BAK/`.
    ///
    /// The operation first creates
    /// `<parent>/.kitchensync/BAK/<timestamp>/` and any missing parent
    /// directories below `<parent>`. Only after that directory exists may it
    /// move `<parent>/<basename>` to
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
    ///
    /// The BAK destination is always below the displaced entry's own parent
    /// directory. It must not place displaced entries in a BAK directory under
    /// the sync root unless the displaced entry's parent is the sync root.
    ///
    /// A successful result means the original path is absent and the displaced
    /// entry is present at the returned BAK destination path. If the displaced
    /// entry is a directory, it is moved as one entry and its complete subtree
    /// remains below the BAK destination.
    ///
    /// Failure is reported when the BAK timestamp directory cannot be created
    /// or when the displaced entry cannot be moved to the BAK destination. The
    /// failure includes the peer identity, original path, BAK timestamp
    /// directory, destination path, and the step that failed so the caller can
    /// report which displacement failed.
    ///
    /// The operation does not choose a different timestamp, choose a different
    /// BAK location, delete the original entry, partially copy directory
    /// contents as a fallback, clean up old BAK directories, recover SWAP
    /// state, update snapshot rows, format output, apply dry-run policy, or
    /// decide which entries should be displaced.
    ///
    /// A completed call is not idempotent for the same request: after success,
    /// the original path is intentionally absent, so a later identical call
    /// must not be treated as another successful displacement of the same
    /// original entry.
    fn displace_to_bak(
        &self,
        request: BakDisplacementRequest,
    ) -> Result<BakDisplacementRecord, BakDisplacementError>;
}
