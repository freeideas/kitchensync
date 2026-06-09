//! Public interface for the `SftpBackend` subproject.
//!
//! `SftpBackend` is the per-scheme adapter the Transport facade delegates to for
//! every `sftp://` peer. It does two things: establish one authenticated,
//! host-key-verified connection to a single already-normalized `sftp://` URL
//! within a bounded handshake, and then carry out the uniform filesystem
//! operation set over that live connection. The facade chooses which URL to try
//! and in what order; this backend answers one URL's connect attempt and one
//! operation at a time and returns the result to the facade.
//!
//! The operation set, return shapes, the `byte_size` rule (`-1` for
//! directories), the non-regular-entry omission rule, and the three error
//! categories are identical to the sibling `LocalBackend`, so a `file://` peer
//! and an `sftp://` peer with identical contents yield identical results.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// The only failure shape any connect attempt or operation may report. A
/// network failure -- a connection drop or a handshake/operation timeout --
/// surfaces as [`BackendError::Io`], never as an SFTP- or SSH-specific error,
/// so callers never match on scheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// The path does not exist, or names a symbolic link or special file.
    NotFound,
    /// The host or filesystem rejected the operation for lack of permission.
    PermissionDenied,
    /// Any other failure, including all network faults (drop, timeout) and
    /// low-level I/O errors.
    Io,
}

/// One immediate child returned by [`SftpConnection::list_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The child's final path component.
    pub name: String,
    /// `true` for a directory, `false` for a regular file.
    pub is_dir: bool,
    /// The child's modification time.
    pub mod_time: SystemTime,
    /// The file size in bytes for a regular file, and `-1` for a directory.
    pub byte_size: i64,
}

/// Metadata returned by [`SftpConnection::stat`] for an existing regular file
/// or directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    /// The entry's modification time.
    pub mod_time: SystemTime,
    /// The file size in bytes for a regular file, and `-1` for a directory.
    pub byte_size: i64,
    /// `true` for a directory, `false` for a regular file.
    pub is_dir: bool,
}

/// An opaque handle to a file opened for reading by
/// [`SftpConnection::open_read`]. The caller passes it back to
/// [`SftpConnection::read`] and finally to [`SftpConnection::close_read`]; it
/// carries no inspectable state across the boundary.
#[derive(Debug)]
pub struct ReadHandle(pub(crate) u64);

/// An opaque handle to a file opened for writing by
/// [`SftpConnection::open_write`]. The caller passes it back to
/// [`SftpConnection::write`] and finally to [`SftpConnection::close_write`]; it
/// carries no inspectable state across the boundary.
#[derive(Debug)]
pub struct WriteHandle(pub(crate) u64);

/// A live, authenticated connection to one `sftp://` peer root, returned by
/// [`SftpBackend::connect`]. Every filesystem call for that peer runs through
/// this handle. Each operation reports failure using only the three
/// [`BackendError`] categories.
pub trait SftpConnection: Send + Sync {
    /// Return each immediate child of `path` with its `name`, `is_dir`,
    /// `mod_time`, and `byte_size` (the file size in bytes for a regular file,
    /// `-1` for a directory). Symbolic links, special files, and any other
    /// non-regular entry are silently omitted, so the result contains only
    /// regular files and directories.
    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, BackendError>;

    /// Return `mod_time`, `byte_size`, and `is_dir` for an existing regular
    /// file or directory. Reports [`BackendError::NotFound`] when `path` does
    /// not exist or names a symbolic link or special file, matching the
    /// `list_dir` omission rule.
    fn stat(&self, path: &str) -> Result<FileMeta, BackendError>;

    /// Open `path` for reading and return a handle for [`read`](Self::read).
    fn open_read(&self, path: &str) -> Result<ReadHandle, BackendError>;

    /// Return the next chunk of up to `max_bytes` bytes from the file behind
    /// `handle`. An empty chunk signals EOF at the end of the file.
    fn read(&self, handle: &ReadHandle, max_bytes: usize) -> Result<Vec<u8>, BackendError>;

    /// Release `handle` obtained from [`open_read`](Self::open_read).
    fn close_read(&self, handle: ReadHandle) -> Result<(), BackendError>;

    /// Create the target file at `path`, creating any missing parent
    /// directories, and return a handle for [`write`](Self::write).
    fn open_write(&self, path: &str) -> Result<WriteHandle, BackendError>;

    /// Append `bytes` to the file behind `handle`.
    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), BackendError>;

    /// Release `handle` obtained from [`open_write`](Self::open_write).
    fn close_write(&self, handle: WriteHandle) -> Result<(), BackendError>;

    /// Move `src` to `dst` when `dst` does not exist. Fails when `dst` already
    /// exists; this never relies on rename-over-existing, so an existing
    /// destination is always a failure rather than an overwrite.
    fn rename(&self, src: &str, dst: &str) -> Result<(), BackendError>;

    /// Create the directory at `path`, creating any missing parent directories.
    fn create_dir(&self, path: &str) -> Result<(), BackendError>;

    /// Remove the file at `path`.
    fn delete_file(&self, path: &str) -> Result<(), BackendError>;

    /// Remove the empty directory at `path`.
    fn delete_dir(&self, path: &str) -> Result<(), BackendError>;

    /// Set the modification time of the file or directory at `path`.
    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<(), BackendError>;
}

/// The per-scheme adapter for `sftp://` peers. `new()` takes no provider
/// handles; the facade holds the resulting `Arc<dyn SftpBackend>` and calls
/// [`connect`](Self::connect) once per winning URL.
pub trait SftpBackend: Send + Sync {
    /// Attempt to connect to one already-normalized `sftp://` `url` and return a
    /// usable [`SftpConnection`], or report this URL as failed so the facade can
    /// try the next one. This decides nothing about a peer's other URLs or
    /// overall reachability; it answers exactly this one URL.
    ///
    /// Authentication tries these credential sources in this fixed order,
    /// skipping any source that is absent and falling through to the next when
    /// the host rejects one: the inline URL password (percent-decoded first, so
    /// `%40` becomes `@` and `%3A` becomes `:`), then the SSH agent named by the
    /// `SSH_AUTH_SOCK` environment variable, then `~/.ssh/id_ed25519`, then
    /// `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`.
    ///
    /// The host key is verified against `~/.ssh/known_hosts`: a host whose key
    /// matches its entry passes, and a host with no matching entry is always
    /// rejected -- an unknown host is never accepted on first sight.
    ///
    /// The handshake is bounded by the effective connection timeout:
    /// `default_timeout_conn` is the bound unless the URL's own `timeout-conn`
    /// query parameter overrides it for this handshake. A handshake that does
    /// not complete within its bound abandons the URL and reports it as failed
    /// rather than waiting on it.
    ///
    /// Peer root handling depends on `dry_run`. When `dry_run` is `false`, a
    /// missing root directory and any missing parents are created, and a URL
    /// whose root cannot be created is failed. When `dry_run` is `true`, a
    /// missing root is not created and a URL whose root does not already exist
    /// is failed for that run, leaving the peer filesystem untouched.
    fn connect(
        &self,
        url: &str,
        default_timeout_conn: Duration,
        dry_run: bool,
    ) -> Result<Arc<dyn SftpConnection>, BackendError>;
}
