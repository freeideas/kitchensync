//! Public specification for the LocalBackend subproject.
//!
//! LocalBackend is the per-scheme adapter that implements Transport's uniform
//! filesystem operation set over a local `file://` peer. The parent Transport
//! chooses a peer's winning `file://` URL and then delegates that peer's root
//! handling and every filesystem operation to this child. Each operation maps to
//! ordinary local filesystem access against the directory named by the URL.
//!
//! The operation set, the return shapes (including the `byte_size` rule of `-1`
//! for a directory), the non-regular-entry omission rule, and the three error
//! categories are identical to SftpBackend's, so a `file://` peer and an
//! `sftp://` peer with identical contents produce identical observations across
//! this boundary.

use std::time::SystemTime;

/// The only failure categories any LocalBackend operation may report (022.17).
///
/// LocalBackend maps every native filesystem error into exactly one of these and
/// never surfaces a scheme-specific or platform-specific error to its caller. A
/// failure that is neither a missing path nor a permission rejection -- including
/// any low-level I/O fault such as a device error -- surfaces as `Io`, the same
/// category SftpBackend uses for a connection drop or timeout, so callers never
/// match on scheme (022.18).
pub enum LocalError {
    /// The path does not exist (022.6), or names a symbolic link or special file
    /// that LocalBackend refuses to treat as a regular entry (022.16).
    NotFound,
    /// The local filesystem rejected the operation for lack of permission.
    PermissionDenied,
    /// Any other failure, including every low-level I/O fault (022.18).
    Io,
}

/// One immediate child returned by [`LocalBackend::list_dir`] (022.2, 022.3,
/// 022.4).
pub struct DirEntry {
    /// The child's own name, with no path prefix.
    pub name: String,
    /// True for a directory, false for a regular file.
    pub is_dir: bool,
    /// The child's modification time.
    pub mod_time: SystemTime,
    /// The file size in bytes for a regular file, or `-1` for a directory.
    pub byte_size: i64,
}

/// The metadata returned by [`LocalBackend::stat`] for an existing entry (022.5).
pub struct Stat {
    /// The entry's modification time.
    pub mod_time: SystemTime,
    /// The file size in bytes for a regular file, or `-1` for a directory.
    pub byte_size: i64,
    /// True for a directory, false for a regular file.
    pub is_dir: bool,
}

/// An opaque handle to an open streaming read, produced by
/// [`LocalBackend::open_read`] and consumed by [`LocalBackend::read`] /
/// [`LocalBackend::close_read`].
pub struct ReadHandle(pub u64);

/// An opaque handle to an open streaming write, produced by
/// [`LocalBackend::open_write`] and consumed by [`LocalBackend::write`] /
/// [`LocalBackend::close_write`].
pub struct WriteHandle(pub u64);

/// The uniform filesystem operations over a single local `file://` peer root.
///
/// `Send + Sync` is required so a single `Arc<dyn LocalBackend>` is a shareable
/// handle. LocalBackend holds no per-run singleton connection state: every
/// path-based operation takes the canonical `file://` `root` Transport handed it,
/// so the backend is a pure function of `(root, path)`. `root` is an
/// already-canonical `file://` URL; LocalBackend never normalizes it.
pub trait LocalBackend: Send + Sync {
    /// Prepare the peer root named by the canonical `file://` `root` URL and
    /// return the per-URL verdict.
    ///
    /// In a normal run (`dry_run` false), create the root directory if it is
    /// missing, creating any missing parent directories along the way (005.9,
    /// 005.11); a root that cannot be created makes this URL fail, returned as
    /// `Err` (005.12).
    ///
    /// In a dry run (`dry_run` true), make no change to the peer filesystem:
    /// never create the root or any parent, and treat a root that does not
    /// already exist as failed -- unreachable for this run -- returned as `Err`
    /// (005.13, 005.14, 024.11).
    ///
    /// Returns `Ok(())` when the root is usable for the run. A failure is reported
    /// through one of [`LocalError`]'s three categories.
    fn open_root(&self, root: &str, dry_run: bool) -> Result<(), LocalError>;

    /// List a directory's immediate children, each as a [`DirEntry`] (022.2,
    /// 022.3, 022.4).
    ///
    /// Silently omits symbolic links, special files, and any other non-regular
    /// entry, so the result contains only regular files and directories (022.15).
    fn list_dir(&self, root: &str, path: &str) -> Result<Vec<DirEntry>, LocalError>;

    /// Return the metadata of an existing regular file or directory (022.5).
    ///
    /// Returns [`LocalError::NotFound`] when the path does not exist (022.6) or
    /// names a symbolic link or special file, matching the `list_dir` omission
    /// rule (022.16).
    fn stat(&self, root: &str, path: &str) -> Result<Stat, LocalError>;

    /// Open a file for streaming read (022.7).
    fn open_read(&self, root: &str, path: &str) -> Result<ReadHandle, LocalError>;

    /// Read the next chunk of at most `max_bytes` bytes from an open file, or
    /// `None` at end of file (022.7).
    fn read(&self, handle: &ReadHandle, max_bytes: usize) -> Result<Option<Vec<u8>>, LocalError>;

    /// Close an open streaming read, releasing its resources (022.7).
    fn close_read(&self, handle: ReadHandle) -> Result<(), LocalError>;

    /// Open a file for streaming write, creating the target file and any missing
    /// parent directories (022.8).
    fn open_write(&self, root: &str, path: &str) -> Result<WriteHandle, LocalError>;

    /// Append the given bytes to an open streaming write (022.8).
    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), LocalError>;

    /// Close an open streaming write, flushing and releasing its resources
    /// (022.8).
    fn close_write(&self, handle: WriteHandle) -> Result<(), LocalError>;

    /// Create the directory and any missing parent directories (022.9).
    fn create_dir(&self, root: &str, path: &str) -> Result<(), LocalError>;

    /// Move `src` to `dst`, only when `dst` does not exist (022.10).
    ///
    /// Fails when `dst` already exists rather than overwriting it; LocalBackend
    /// never relies on rename-over-existing, leaving any staged replacement to the
    /// callers that need it (022.11).
    fn rename(&self, root: &str, src: &str, dst: &str) -> Result<(), LocalError>;

    /// Remove a file (022.12).
    fn delete_file(&self, root: &str, path: &str) -> Result<(), LocalError>;

    /// Remove an empty directory (022.13).
    fn delete_dir(&self, root: &str, path: &str) -> Result<(), LocalError>;

    /// Set the modification time of a file or directory (022.14).
    fn set_mod_time(&self, root: &str, path: &str, time: SystemTime) -> Result<(), LocalError>;
}
