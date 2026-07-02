use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTransportRoot {
    pub local_peer_root_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTransportDirEntry {
    pub child_name: String,
    pub metadata: LocalTransportMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTransportMetadata {
    pub modification_time: SystemTime,
    pub byte_size: i64,
    pub entry_type: LocalTransportEntryType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalTransportEntryType {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalTransportReadHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalTransportWriteHandle(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalTransportReadResult {
    Bytes(Vec<u8>),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalTransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

pub type LocalTransportResult<T> = Result<T, LocalTransportErrorCategory>;

pub trait LocalTransportOperations: Send + Sync {
    /// Lists only the immediate regular-file and directory children of `path`
    /// under the connected local root.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root.
    /// Regular file children are returned with their child name, modification
    /// time, byte size, and `File` type. Directory children are returned with
    /// their child name, modification time, byte size `-1`, and `Directory`
    /// type. Symbolic links, devices, FIFOs, sockets, and other non-regular
    /// non-directory entries are omitted. The specification does not require a
    /// stable ordering, so callers must not depend on the returned order.
    /// Missing paths and entries treated as absent return `NotFound`; local
    /// access-denied failures return `PermissionDenied`; other local
    /// filesystem failures return `IoError`.
    fn list_dir(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<Vec<LocalTransportDirEntry>>;

    /// Returns metadata for an existing regular file or directory at `path`.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. Regular files
    /// return their modification time, byte size, and `File` type. Directories
    /// return their modification time, byte size `-1`, and `Directory` type.
    /// A missing path, symbolic link, device, FIFO, socket, or other
    /// non-regular non-directory entry returns `NotFound`. Local
    /// access-denied failures return `PermissionDenied`; other local
    /// filesystem failures return `IoError`.
    fn stat(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<LocalTransportMetadata>;

    /// Opens an existing regular file at `path` for streaming reads.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. The returned handle
    /// is owned by this child and later `read` calls consume bytes from that
    /// handle's current position. Missing paths, directories, symbolic links,
    /// and non-regular entries return `NotFound`. Local access-denied failures
    /// return `PermissionDenied`; other local filesystem failures return
    /// `IoError`.
    fn open_read(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<LocalTransportReadHandle>;

    /// Reads the next file-content bytes from an open read handle.
    ///
    /// Each successful `Bytes` result contains no more than `max_bytes` bytes
    /// from the handle's current position and advances that position by the
    /// number of bytes returned. After all file content has been returned, the
    /// operation returns `Eof`. The handle must be one previously returned by
    /// `open_read` and still owned by this child. Local access-denied failures
    /// return `PermissionDenied`; other local filesystem failures return
    /// `IoError`.
    fn read(
        &self,
        handle: LocalTransportReadHandle,
        max_bytes: usize,
    ) -> LocalTransportResult<LocalTransportReadResult>;

    /// Closes an open read handle owned by this child.
    ///
    /// Closing releases the local file handle associated with the supplied
    /// read handle. The operation does not mutate peer file content. Other
    /// local filesystem failures while closing return `IoError`.
    fn close_read(&self, handle: LocalTransportReadHandle) -> LocalTransportResult<()>;

    /// Opens `path` for streaming writes, creating the target file and any
    /// missing parent directories.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. The returned handle
    /// is owned by this child and later `write` calls send bytes to that local
    /// file handle. Local access-denied failures return `PermissionDenied`;
    /// other local filesystem failures return `IoError`.
    fn open_write(
        &self,
        root: &LocalTransportRoot,
        path: &str,
    ) -> LocalTransportResult<LocalTransportWriteHandle>;

    /// Writes `bytes` to an open write handle in call order.
    ///
    /// The handle must be one previously returned by `open_write` and still
    /// owned by this child. Successful calls write the supplied bytes at the
    /// current stream position for that handle; callers that need a specific
    /// final byte sequence must call `write` in that sequence. Local
    /// access-denied failures return `PermissionDenied`; other local
    /// filesystem failures return `IoError`.
    fn write(
        &self,
        handle: LocalTransportWriteHandle,
        bytes: &[u8],
    ) -> LocalTransportResult<()>;

    /// Flushes pending local file data and closes an open write handle.
    ///
    /// A successful close means pending data for the handle has been flushed
    /// before the handle is released. The handle must be one previously
    /// returned by `open_write` and still owned by this child. Local
    /// access-denied failures return `PermissionDenied`; other local
    /// filesystem failures return `IoError`.
    fn close_write(&self, handle: LocalTransportWriteHandle) -> LocalTransportResult<()>;

    /// Moves an entry from `src` to `dst` within the same local filesystem when
    /// `dst` does not already exist.
    ///
    /// Both paths are peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. This operation does
    /// not require overwrite of an existing destination, so callers that
    /// replace data must use a sequence that works when destination overwrite
    /// is rejected. Missing source paths and entries treated as absent return
    /// `NotFound`; local access-denied failures return `PermissionDenied`;
    /// other local filesystem failures return `IoError`.
    fn rename(
        &self,
        root: &LocalTransportRoot,
        src: &str,
        dst: &str,
    ) -> LocalTransportResult<()>;

    /// Removes a local file at `path`.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. Missing paths,
    /// directories, symbolic links, and non-regular entries return `NotFound`.
    /// Local access-denied failures return `PermissionDenied`; other local
    /// filesystem failures return `IoError`.
    fn delete_file(&self, root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()>;

    /// Creates a local directory at `path` and any missing parent directories.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. Local
    /// access-denied failures return `PermissionDenied`; other local
    /// filesystem failures return `IoError`.
    fn create_dir(&self, root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()>;

    /// Removes an empty local directory at `path`.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. Missing paths,
    /// regular files, symbolic links, and non-directory entries return
    /// `NotFound`. A non-empty directory is a local filesystem failure and
    /// returns `IoError`. Local access-denied failures return
    /// `PermissionDenied`.
    fn delete_dir(&self, root: &LocalTransportRoot, path: &str) -> LocalTransportResult<()>;

    /// Updates the modification time of a local regular file or directory at
    /// `path`.
    ///
    /// The path is peer-relative and must be resolved inside
    /// `root.local_peer_root_path`; the operation must not escape that root
    /// and must not follow symbolic links as peer entries. Missing paths,
    /// symbolic links, devices, FIFOs, sockets, and other non-regular
    /// non-directory entries return `NotFound`. Local access-denied failures
    /// return `PermissionDenied`; other local filesystem failures return
    /// `IoError`.
    fn set_mod_time(
        &self,
        root: &LocalTransportRoot,
        path: &str,
        time: SystemTime,
    ) -> LocalTransportResult<()>;
}
