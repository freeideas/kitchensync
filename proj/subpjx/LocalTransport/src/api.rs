use std::path::PathBuf;
use std::time::SystemTime;

use peertransportsurface::{
    ConnectedPeerRoot, PeerDirectoryEntry, PeerMetadata, PeerReadChunk, PeerReadHandle,
    PeerTransportError, PeerWriteHandle,
};

pub struct LocalConnectionRequest {
    pub root_path: PathBuf,
    pub create_missing_root: bool,
}

pub trait LocalTransport: Send + Sync {
    /// Connects one local candidate that has already been identified as a
    /// `file://` URL or bare path peer by the command-line and URL layers.
    /// Connection timeout and idle keep-alive settings are not inputs to this
    /// operation and must not delay, cancel, or otherwise affect local
    /// connection establishment. When `create_missing_root` is true, a missing
    /// root and any missing parents are created before success is reported. When
    /// creation is not allowed or creation fails, the candidate fails startup
    /// through the returned transport-neutral error category. The returned root
    /// is the only base for later root-relative operations during the run.
    fn connect(
        &self,
        request: LocalConnectionRequest,
    ) -> Result<ConnectedPeerRoot, PeerTransportError>;

    /// Lists exactly the immediate children of `path` under the connected root.
    /// The operation does not recurse, preserves each reported child name
    /// exactly as the host filesystem reports it, and reports failures only as
    /// `not found`, `permission denied`, or `I/O error`.
    fn list_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError>;

    /// Returns metadata for a regular file or directory at `path` under the
    /// connected root. Missing paths and local entries that cannot be exposed
    /// through the shared peer transport surface fail as `not found`; all other
    /// failures use only the transport-neutral categories.
    fn stat(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerMetadata, PeerTransportError>;

    /// Opens a regular file at `path` under the connected root for streaming
    /// reads. The path is resolved only against the connected root supplied at
    /// startup; later failures use only the transport-neutral categories.
    fn open_read(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerReadHandle, PeerTransportError>;

    /// Returns the next byte chunk from an open read handle, in file order, or
    /// EOF. The returned byte chunk must not exceed `max_bytes`. After the
    /// handle is closed, later reads through that handle are outside this
    /// interface's guarantees.
    fn read(
        &self,
        handle: &mut PeerReadHandle,
        max_bytes: usize,
    ) -> Result<PeerReadChunk, PeerTransportError>;

    /// Closes an open read handle. After this operation returns, later reads
    /// through that handle are outside this interface's guarantees.
    fn close_read(&self, handle: PeerReadHandle) -> Result<(), PeerTransportError>;

    /// Opens `path` under the connected root for streaming writes. Opening a
    /// writer creates the target file and any needed parent directories before
    /// bytes are written. Failures use only the transport-neutral categories.
    fn open_write(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerWriteHandle, PeerTransportError>;

    /// Writes the supplied bytes to the open write handle in call order. After
    /// the handle is closed, later writes through that handle are outside this
    /// interface's guarantees.
    fn write(
        &self,
        handle: &mut PeerWriteHandle,
        bytes: &[u8],
    ) -> Result<(), PeerTransportError>;

    /// Finalizes an open write handle so later peer reads return the written
    /// bytes, or returns a transport-neutral failure category explaining why
    /// finalization failed. After this operation returns, later writes through
    /// that handle are outside this interface's guarantees.
    fn close_write(&self, handle: PeerWriteHandle) -> Result<(), PeerTransportError>;

    /// Moves `src` to the non-existing `dst` under the same connected root.
    /// This operation guarantees only the non-overwrite rename shape used by
    /// the shared peer transport surface; callers must not depend on replacing
    /// an existing destination.
    fn rename(
        &self,
        peer: &ConnectedPeerRoot,
        src: &str,
        dst: &str,
    ) -> Result<(), PeerTransportError>;

    /// Removes the file at `path` under the connected root. The operation does
    /// not switch roots or retry another candidate URL, and failures use only
    /// the transport-neutral categories.
    fn delete_file(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Creates the directory at `path` under the connected root and any needed
    /// parent directories. The operation does not switch roots or retry another
    /// candidate URL, and failures use only the transport-neutral categories.
    fn create_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Removes the empty directory at `path` under the connected root. The
    /// operation does not switch roots or retry another candidate URL, and
    /// failures use only the transport-neutral categories.
    fn delete_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Sets the modification time of an existing file or directory at `path`
    /// under the connected root. The operation does not switch roots or retry
    /// another candidate URL, and failures use only the transport-neutral
    /// categories.
    fn set_mod_time(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
        mod_time: SystemTime,
    ) -> Result<(), PeerTransportError>;
}
