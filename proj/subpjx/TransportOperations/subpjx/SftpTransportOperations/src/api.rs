#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpTransportEntry {
    pub name: String,
    pub metadata: SftpTransportMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpTransportMetadata {
    pub modification_time: SftpTransportModificationTime,
    pub byte_size: i64,
    pub entry_type: SftpTransportEntryType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SftpTransportModificationTime {
    pub seconds_since_unix_epoch: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SftpTransportEntryType {
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpTransportReadHandle {
    pub id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpTransportWriteHandle {
    pub id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpTransportReadChunk {
    pub bytes: Vec<u8>,
    pub eof: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpTransportError {
    pub category: SftpTransportErrorCategory,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SftpTransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

pub trait SftpTransportOperations: Send + Sync {
    /// Lists only the immediate regular-file and directory children of the
    /// peer-relative directory path through the established SFTP connection.
    /// The path is interpreted inside the connected SFTP peer root.
    ///
    /// Returned names are child names, not full paths. Regular files report
    /// their modification time, byte size, and `File` type. Directories report
    /// their modification time, byte size `-1`, and `Directory` type. Symbolic
    /// links and non-regular non-directory filesystem objects are omitted.
    ///
    /// Missing paths and paths treated as absent return `NotFound`. Access
    /// failures return `PermissionDenied`. SFTP, SSH, socket, timeout, lost
    /// channel, and other transport failures return `IoError`.
    fn list_dir(&self, path: &str) -> Result<Vec<SftpTransportEntry>, SftpTransportError>;

    /// Reads metadata for an existing regular file or directory at the
    /// peer-relative path through the established SFTP connection. The path is
    /// interpreted inside the connected SFTP peer root.
    ///
    /// The returned metadata uses the same shape as directory listing entries:
    /// regular files report modification time, byte size, and `File` type;
    /// directories report modification time, byte size `-1`, and `Directory`
    /// type. A missing path, symbolic link, or non-regular non-directory
    /// filesystem object returns `NotFound`.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn stat(&self, path: &str) -> Result<SftpTransportMetadata, SftpTransportError>;

    /// Opens an existing regular file at the peer-relative path for streaming
    /// reads through the established SFTP connection. The path is interpreted
    /// inside the connected SFTP peer root.
    ///
    /// The returned handle belongs to the SFTP connection that created it and
    /// is valid only for later `read` and `close_read` calls on that
    /// connection. Missing paths, symbolic links, directories, and other
    /// non-regular file objects return `NotFound`.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn open_read(&self, path: &str) -> Result<SftpTransportReadHandle, SftpTransportError>;

    /// Returns up to `max_bytes` of the next file-content bytes from an open
    /// SFTP read handle.
    ///
    /// Bytes are returned in file order for that handle. The returned chunk
    /// contains only file content bytes. EOF is reported after all file content
    /// has been returned. A handle that is absent, already closed, or not owned
    /// by this SFTP connection fails with one of this interface's error
    /// categories.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn read(
        &self,
        handle: &SftpTransportReadHandle,
        max_bytes: usize,
    ) -> Result<SftpTransportReadChunk, SftpTransportError>;

    /// Closes an open SFTP read handle that belongs to this connection.
    ///
    /// After a successful close, the handle must no longer be usable for
    /// reading. A handle that is absent, already closed, or not owned by this
    /// SFTP connection fails with one of this interface's error categories.
    ///
    /// SFTP, SSH, socket, timeout, lost channel, and other transport failures
    /// return `IoError`.
    fn close_read(&self, handle: SftpTransportReadHandle) -> Result<(), SftpTransportError>;

    /// Opens the peer-relative path for streaming writes through the
    /// established SFTP connection. The path is interpreted inside the
    /// connected SFTP peer root.
    ///
    /// The operation creates the target file when it does not exist and
    /// creates missing parent directories required by the parent operation
    /// surface. The returned handle belongs to the SFTP connection that
    /// created it and is valid only for later `write` and `close_write` calls
    /// on that connection.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn open_write(&self, path: &str) -> Result<SftpTransportWriteHandle, SftpTransportError>;

    /// Writes the supplied bytes to an open SFTP write handle.
    ///
    /// Bytes supplied across calls for the same handle are sent in call order.
    /// A handle that is absent, already closed, or not owned by this SFTP
    /// connection fails with one of this interface's error categories.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn write(
        &self,
        handle: &SftpTransportWriteHandle,
        bytes: &[u8],
    ) -> Result<(), SftpTransportError>;

    /// Flushes and closes an open SFTP write handle that belongs to this
    /// connection.
    ///
    /// Success is reported only after pending data has been flushed and the
    /// remote file handle has been closed. After a successful close, the handle
    /// must no longer be usable for writing. A handle that is absent, already
    /// closed, or not owned by this SFTP connection fails with one of this
    /// interface's error categories.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn close_write(&self, handle: SftpTransportWriteHandle) -> Result<(), SftpTransportError>;

    /// Moves an entry from `src` to `dst` within the same connected SFTP
    /// filesystem. Both paths are interpreted inside the connected SFTP peer
    /// root.
    ///
    /// The operation is required only when `dst` does not already exist and
    /// must not rely on remote rename-over-existing behavior. Missing source
    /// paths and paths treated as absent return `NotFound`.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn rename(&self, src: &str, dst: &str) -> Result<(), SftpTransportError>;

    /// Removes a regular file at the peer-relative path through the
    /// established SFTP connection. The path is interpreted inside the
    /// connected SFTP peer root.
    ///
    /// Missing paths, symbolic links, directories, and other non-regular file
    /// objects return `NotFound`.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn delete_file(&self, path: &str) -> Result<(), SftpTransportError>;

    /// Creates a directory at the peer-relative path and any missing parent
    /// directories through the established SFTP connection. The path is
    /// interpreted inside the connected SFTP peer root.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn create_dir(&self, path: &str) -> Result<(), SftpTransportError>;

    /// Removes an empty directory at the peer-relative path through the
    /// established SFTP connection. The path is interpreted inside the
    /// connected SFTP peer root.
    ///
    /// Missing paths, symbolic links, regular files, and non-directory objects
    /// return `NotFound`.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn delete_dir(&self, path: &str) -> Result<(), SftpTransportError>;

    /// Updates the modification time of a regular file or directory at the
    /// peer-relative path through the established SFTP connection. The path is
    /// interpreted inside the connected SFTP peer root.
    ///
    /// Missing paths, symbolic links, and non-regular non-directory filesystem
    /// objects return `NotFound`. The operation must not turn symbolic links
    /// into traversable paths.
    ///
    /// Access failures return `PermissionDenied`. SFTP, SSH, socket, timeout,
    /// lost channel, and other transport failures return `IoError`.
    fn set_mod_time(
        &self,
        path: &str,
        time: SftpTransportModificationTime,
    ) -> Result<(), SftpTransportError>;
}
