use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct SnapshotPeerFilesConnectedPeer {
    pub identity: String,
    pub winning_url: String,
    pub scheme: SnapshotPeerFilesPeerScheme,
    pub handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPeerFilesPeerScheme {
    File,
    Sftp,
}

#[derive(Clone)]
pub struct SnapshotPeerFilesStartupRequest {
    pub peer: SnapshotPeerFilesConnectedPeer,
    pub local_snapshot_directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotPeerFilesStartupResult {
    RecoveredAndDownloaded {
        peer_identity: String,
        local_snapshot_path: PathBuf,
    },
    RecoveredWithNewEmptyLocalSnapshot {
        peer_identity: String,
        local_snapshot_path: PathBuf,
    },
    Unavailable(SnapshotPeerFilesStartupFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotPeerFilesStartupFailure {
    pub peer_identity: String,
    pub kind: SnapshotPeerFilesStartupFailureKind,
    pub transport_error: Option<SnapshotPeerFilesTransportErrorKind>,
    pub details: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPeerFilesStartupFailureKind {
    SwapRecoveryFailed,
    SnapshotDownloadFailed,
    LocalDatabaseFailed,
}

#[derive(Clone)]
pub struct SnapshotPeerFilesUploadRequest {
    pub peer: SnapshotPeerFilesConnectedPeer,
    pub local_snapshot_path: PathBuf,
}

pub type SnapshotPeerFilesUploadResult = Result<(), SnapshotPeerFilesUploadFailure>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotPeerFilesUploadFailure {
    pub peer_identity: String,
    pub kind: SnapshotPeerFilesUploadFailureKind,
    pub transport_error: Option<SnapshotPeerFilesTransportErrorKind>,
    pub details: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPeerFilesUploadFailureKind {
    PrepareLocalDatabaseFailed,
    WriteSwapNewFailed,
    MoveLiveToSwapOldFailed,
    MoveNewToLiveFailed,
    RemoveSwapOldFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPeerFilesTransportErrorKind {
    NotFound,
    PermissionDenied,
    IoError,
}

pub trait SnapshotPeerFiles: Send + Sync {
    /// Performs normal-run snapshot startup file work for one connected peer.
    ///
    /// This operation first recovers peer-side snapshot replacement state under
    /// `.kitchensync/SWAP/snapshot.db/` before deciding whether the peer has
    /// live snapshot history. Recovery uses only the live path
    /// `.kitchensync/snapshot.db`, SWAP `new` path
    /// `.kitchensync/SWAP/snapshot.db/new`, and SWAP `old` path
    /// `.kitchensync/SWAP/snapshot.db/old`. If `old` and live both exist, live
    /// remains in place and `old` plus any `new` are deleted. If `old` and
    /// `new` exist while live is missing, `new` becomes live and `old` is
    /// deleted. If only `old` exists, `old` becomes live. If `new` and live
    /// exist while `old` is missing, live remains in place and `new` is
    /// deleted. If only `new` exists, `new` becomes live.
    ///
    /// If recovery fails, the result is `Unavailable` with
    /// `SwapRecoveryFailed`, and any remaining SWAP state is left for a later
    /// normal startup. After successful recovery, an existing live
    /// `.kitchensync/snapshot.db` is downloaded to
    /// `local_snapshot_directory/snapshot.db` and returned as
    /// `RecoveredAndDownloaded`. A missing live snapshot is not a peer failure:
    /// this operation creates a new empty local
    /// `local_snapshot_directory/snapshot.db` through the local snapshot
    /// database owner and returns `RecoveredWithNewEmptyLocalSnapshot`.
    ///
    /// Download errors other than not found make only this peer unavailable
    /// with `SnapshotDownloadFailed`. Local empty-database creation failure
    /// makes only this peer unavailable with `LocalDatabaseFailed`. Peer SQLite
    /// sidecar files are not downloaded, uploaded, or treated as snapshot
    /// state. This is a normal-run operation; dry-run callers must not use it
    /// to perform peer-side SWAP recovery.
    fn start_normal_peer_snapshot(
        &self,
        request: SnapshotPeerFilesStartupRequest,
    ) -> SnapshotPeerFilesStartupResult;

    /// Uploads one closed local temporary `snapshot.db` to one connected peer.
    ///
    /// The caller must invoke this only in a normal run, after all enqueued
    /// file copies for the run have completed and the local snapshot database
    /// at `local_snapshot_path` has been closed for filesystem upload. The
    /// upload reads bytes from that closed `snapshot.db` file, never from a
    /// live SQLite connection, and sends only `snapshot.db` as peer snapshot
    /// state. SQLite sidecar files are not uploaded.
    ///
    /// The replacement sequence writes the database to
    /// `.kitchensync/SWAP/snapshot.db/new`, closes peer-side `new`, moves an
    /// existing live `.kitchensync/snapshot.db` to
    /// `.kitchensync/SWAP/snapshot.db/old`, moves `new` into the live snapshot
    /// path, and deletes `old` after `new` has become live. The sequence must
    /// replace an existing live snapshot even when the transport rejects
    /// `rename(src, dst)` over an existing destination.
    ///
    /// If upload fails before SWAP `old` exists, the live snapshot is not
    /// removed and leftover `new` is left for the next normal startup
    /// recovery. If upload fails after SWAP `old` exists, the remaining
    /// peer-side snapshot SWAP state is retained for the next normal startup
    /// recovery. A completed upload is one that has moved SWAP `new` into the
    /// live snapshot path; when overlapping normal runs upload to the same
    /// peer, the live snapshot must reflect the last completed upload.
    fn upload_normal_peer_snapshot(
        &self,
        request: SnapshotPeerFilesUploadRequest,
    ) -> SnapshotPeerFilesUploadResult;
}
