use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone)]
pub enum TransportPeerHandle {
    File {
        root: PathBuf,
        handle: Arc<dyn Any + Send + Sync>,
    },
    Sftp {
        root: String,
        handle: Arc<dyn Any + Send + Sync>,
    },
}

pub struct TransportReadHandle {
    pub(crate) handle: Arc<dyn Any + Send + Sync>,
}

pub struct TransportWriteHandle {
    pub(crate) handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportDirectoryEntry {
    pub name: String,
    pub metadata: TransportMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportMetadata {
    pub modification_time: SystemTime,
    pub byte_size: i64,
    pub entry_type: TransportEntryType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportEntryType {
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportReadResult {
    Bytes(Vec<u8>),
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError {
    pub category: TransportErrorCategory,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

pub trait TransportOperations: Send + Sync {
    /// Lists only the immediate children of `path` under the connected peer
    /// root.
    ///
    /// Returned names are child names, not full peer-relative paths. Regular
    /// files are returned with their modification time, byte size, and
    /// `TransportEntryType::File`. Directories are returned with their
    /// modification time, byte size `-1`, and
    /// `TransportEntryType::Directory`. Symbolic links, devices, FIFOs,
    /// sockets, and any other entry that is neither a regular file nor a
    /// directory are omitted. Paths are interpreted inside the connected peer
    /// root for both `file://` and `sftp://` peers, and symbolic links must not
    /// be followed to escape that root.
    fn list_dir(
        &self,
        peer: &TransportPeerHandle,
        path: &str,
    ) -> Result<Vec<TransportDirectoryEntry>, TransportError>;

    /// Returns metadata for an existing regular file or directory at `path`.
    ///
    /// Metadata uses the same rules as `list_dir`: regular files report their
    /// byte size and `TransportEntryType::File`, while directories report byte
    /// size `-1` and `TransportEntryType::Directory`. A missing path, symbolic
    /// link, device, FIFO, socket, or other non-regular non-directory entry is
    /// reported as `TransportErrorCategory::NotFound`. Paths are interpreted
    /// inside the connected peer root, and symbolic links must not be followed
    /// to escape that root.
    fn stat(
        &self,
        peer: &TransportPeerHandle,
        path: &str,
    ) -> Result<TransportMetadata, TransportError>;

    /// Opens an existing regular file for streaming reads from `path`.
    ///
    /// The returned handle belongs to the supplied connected peer and is used
    /// only with `read` and `close_read`. Opening a missing path or anything
    /// other than a regular file is reported with the transport error category
    /// that matches the underlying failure. Paths are interpreted inside the
    /// connected peer root, and symbolic links must not be followed to escape
    /// that root.
    fn open_read(
        &self,
        peer: &TransportPeerHandle,
        path: &str,
    ) -> Result<TransportReadHandle, TransportError>;

    /// Reads up to `max_bytes` of the next file-content bytes from an open
    /// read handle.
    ///
    /// Repeated successful reads on the same handle return file content in
    /// order from the current handle position. After all file content has been
    /// returned, this method returns `TransportReadResult::Eof`. The bytes
    /// returned are only file content bytes; EOF is reported separately from a
    /// successful byte chunk.
    fn read(
        &self,
        handle: &TransportReadHandle,
        max_bytes: usize,
    ) -> Result<TransportReadResult, TransportError>;

    /// Closes an open read handle.
    ///
    /// After this operation succeeds, the handle is no longer an open peer read
    /// stream. Closing a read handle does not modify peer filesystem content.
    fn close_read(&self, handle: TransportReadHandle) -> Result<(), TransportError>;

    /// Opens `path` for streaming writes.
    ///
    /// The operation creates the target file when it does not exist and creates
    /// any missing parent directories for the target file. The returned handle
    /// belongs to the supplied connected peer and is used only with `write` and
    /// `close_write`. Paths are interpreted inside the connected peer root, and
    /// symbolic links must not be followed to escape that root.
    fn open_write(
        &self,
        peer: &TransportPeerHandle,
        path: &str,
    ) -> Result<TransportWriteHandle, TransportError>;

    /// Writes the supplied bytes to an open write handle.
    ///
    /// Successful calls append their bytes to the stream in call order for that
    /// handle. For SFTP peers, bytes are sent through the established SSH/SFTP
    /// connection for the peer. Network failures during SFTP transport are
    /// reported as `TransportErrorCategory::IoError`.
    fn write(&self, handle: &TransportWriteHandle, bytes: &[u8]) -> Result<(), TransportError>;

    /// Flushes and closes an open write handle.
    ///
    /// A successful close makes all previously successful writes for that
    /// handle durable according to the underlying local filesystem or SFTP
    /// server behavior before the handle is released. SFTP network failures
    /// while flushing or closing are reported as
    /// `TransportErrorCategory::IoError`.
    fn close_write(&self, handle: TransportWriteHandle) -> Result<(), TransportError>;

    /// Moves `src` to `dst` within the same connected peer filesystem.
    ///
    /// The operation is only required to succeed when `dst` does not already
    /// exist. Callers that replace data must use a sequence that works when
    /// destination overwrite is rejected. Both paths are interpreted inside the
    /// connected peer root and must not escape that root through symbolic
    /// links.
    fn rename(
        &self,
        peer: &TransportPeerHandle,
        src: &str,
        dst: &str,
    ) -> Result<(), TransportError>;

    /// Removes a regular file at `path`.
    ///
    /// The path is interpreted inside the connected peer root. Failures are
    /// reported only with `TransportErrorCategory::NotFound`,
    /// `TransportErrorCategory::PermissionDenied`, or
    /// `TransportErrorCategory::IoError`, with the same category meaning the
    /// same thing for local and SFTP peers.
    fn delete_file(&self, peer: &TransportPeerHandle, path: &str) -> Result<(), TransportError>;

    /// Creates a directory at `path` and any missing parent directories.
    ///
    /// The path is interpreted inside the connected peer root. The operation
    /// uses the local filesystem for `file://` peers and the established
    /// SSH/SFTP connection for `sftp://` peers. SFTP network failures are
    /// reported as `TransportErrorCategory::IoError`.
    fn create_dir(&self, peer: &TransportPeerHandle, path: &str) -> Result<(), TransportError>;

    /// Removes an empty directory at `path`.
    ///
    /// The operation removes only an empty directory in the connected peer
    /// filesystem. The path is interpreted inside the connected peer root, and
    /// failures use the shared transport error categories for both local and
    /// SFTP peers.
    fn delete_dir(&self, peer: &TransportPeerHandle, path: &str) -> Result<(), TransportError>;

    /// Updates the modification time of a regular file or directory at `path`.
    ///
    /// The path is interpreted inside the connected peer root. The operation
    /// applies to regular files and directories using the same entry treatment
    /// as `stat`; symbolic links and other non-regular non-directory entries
    /// are treated as absent. SFTP network failures are reported as
    /// `TransportErrorCategory::IoError`.
    fn set_mod_time(
        &self,
        peer: &TransportPeerHandle,
        path: &str,
        time: SystemTime,
    ) -> Result<(), TransportError>;
}
