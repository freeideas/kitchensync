use std::any::Any;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone)]
pub struct ConnectedPeerRoot {
    pub handle: Arc<dyn Any + Send + Sync>,
}

pub struct PeerReadHandle {
    pub handle: Box<dyn Any + Send>,
}

pub struct PeerWriteHandle {
    pub handle: Box<dyn Any + Send>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerDirectoryEntry {
    pub child_name: String,
    pub is_dir: bool,
    pub mod_time: SystemTime,
    pub byte_size: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerMetadata {
    pub is_dir: bool,
    pub mod_time: SystemTime,
    pub byte_size: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerReadChunk {
    Bytes(Vec<u8>),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerTransportError {
    NotFound,
    PermissionDenied,
    IoError,
}

pub trait PeerTransportSurface: Send + Sync {
    /// Lists exactly the immediate children of `path` under the already
    /// connected peer root. The operation does not recurse, does not retry a
    /// fallback URL, and does not expose transport-specific paths or sessions.
    /// Each returned child name is preserved exactly as the peer filesystem
    /// reports it, without case changes, Unicode normalization, separator
    /// rewriting, or other canonicalization. The result omits symbolic links,
    /// special files, device files, FIFOs, sockets, and every other
    /// non-regular entry type. Directory entries have `byte_size = -1`.
    /// Failures crossing this boundary are only `not found`, `permission
    /// denied`, or `I/O error`.
    fn list_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError>;

    /// Returns metadata for a regular file or directory at `path` under the
    /// already connected peer root. The operation does not retry a fallback
    /// URL and does not expose transport-specific paths or sessions.
    /// Directories report `byte_size = -1`; regular files report their byte
    /// size. Missing paths, symbolic links, special files, device files, FIFOs,
    /// sockets, and every other non-regular entry type fail as `not found`.
    /// All other failures crossing this boundary are only `permission denied`
    /// or `I/O error`.
    fn stat(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerMetadata, PeerTransportError>;

    /// Opens a regular file at `path` under the already connected peer root
    /// for streaming reads. The operation is scoped only to that root, does not
    /// retry a fallback URL, and returns only the shared error categories.
    fn open_read(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerReadHandle, PeerTransportError>;

    /// Returns the next byte chunk from an open read handle in file order, or
    /// EOF. A returned byte chunk must not exceed `max_bytes`. After the handle
    /// is closed, later reads through that handle are outside this surface's
    /// guarantees. Failures crossing this boundary use only the shared error
    /// categories.
    fn read(
        &self,
        handle: &mut PeerReadHandle,
        max_bytes: usize,
    ) -> Result<PeerReadChunk, PeerTransportError>;

    /// Closes an open read handle. After this operation returns, later reads
    /// through that handle are outside this surface's guarantees. Failures
    /// crossing this boundary use only the shared error categories.
    fn close_read(&self, handle: PeerReadHandle) -> Result<(), PeerTransportError>;

    /// Opens `path` under the already connected peer root for streaming
    /// writes. Opening a writer creates the target file and any needed parent
    /// directories before bytes are written. The operation is scoped only to
    /// that root, does not retry a fallback URL, and returns only the shared
    /// error categories.
    fn open_write(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerWriteHandle, PeerTransportError>;

    /// Writes the supplied bytes to the open write handle in call order. After
    /// the handle is closed, later writes through that handle are outside this
    /// surface's guarantees. Failures crossing this boundary use only the
    /// shared error categories.
    fn write(
        &self,
        handle: &mut PeerWriteHandle,
        bytes: &[u8],
    ) -> Result<(), PeerTransportError>;

    /// Finalizes an open write handle so later peer reads return the written
    /// bytes, or returns a shared failure category explaining why finalization
    /// failed. After this operation returns, later writes through that handle
    /// are outside this surface's guarantees.
    fn close_write(&self, handle: PeerWriteHandle) -> Result<(), PeerTransportError>;

    /// Moves `src` to a non-existing `dst` on the same filesystem under the
    /// already connected peer root. This surface guarantees only the
    /// non-overwrite rename shape; callers must not depend on replacing an
    /// existing destination or on transport-specific overwrite behavior. The
    /// operation does not retry a fallback URL and returns only the shared
    /// error categories.
    fn rename(
        &self,
        peer: &ConnectedPeerRoot,
        src: &str,
        dst: &str,
    ) -> Result<(), PeerTransportError>;

    /// Removes the file at `path` under the already connected peer root. The
    /// operation does not retry a fallback URL, does not remove directories,
    /// and returns only the shared error categories.
    fn delete_file(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Creates the directory at `path` under the already connected peer root
    /// and any needed parent directories. The operation does not retry a
    /// fallback URL and returns only the shared error categories.
    fn create_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Removes the empty directory at `path` under the already connected peer
    /// root. The operation does not retry a fallback URL, does not remove
    /// non-empty directories, and returns only the shared error categories.
    fn delete_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Sets the modification time of an existing file or directory at `path`
    /// under the already connected peer root. The stored value is the peer
    /// modification time used by the rest of the product for comparison,
    /// snapshot storage, and copy preservation. The operation does not retry a
    /// fallback URL and returns only the shared error categories.
    fn set_mod_time(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
        mod_time: SystemTime,
    ) -> Result<(), PeerTransportError>;
}
