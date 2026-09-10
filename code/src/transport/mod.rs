//! Peer transports. Every peer, local or SFTP, is reached through the same
//! `Transport` trait so the sync engine never cares about the scheme.
//! See specs/sync.md, section "Peer Transports".

pub mod local;
pub mod sftp;

use std::fmt;
use std::time::SystemTime;

/// Error categories shared by all transports (specs/sync.md, "Error Semantics").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    Io,
}

impl ErrorKind {
    /// Wire name used in diagnostics, e.g. `permission_denied`.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::PermissionDenied => "permission_denied",
            ErrorKind::Io => "io_error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransportError {
    pub kind: ErrorKind,
    pub message: String,
}

impl TransportError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        TransportError { kind, message: message.into() }
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, message)
    }
    pub fn is_not_found(&self) -> bool {
        self.kind == ErrorKind::NotFound
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind as K;
        let kind = match e.kind() {
            K::NotFound => ErrorKind::NotFound,
            K::PermissionDenied => ErrorKind::PermissionDenied,
            _ => ErrorKind::Io,
        };
        TransportError { kind, message: e.to_string() }
    }
}

pub type Result<T> = std::result::Result<T, TransportError>;

/// One entry from `list_dir` or `stat`. Only regular files and directories are
/// ever returned; symlinks and special files are omitted by the transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub mod_time: SystemTime,
    /// Bytes for files, -1 for directories.
    pub byte_size: i64,
}

/// Streaming read handle.
pub trait ReadHandle: Send {
    /// Fill `buf`, returning bytes read; 0 means EOF.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
}

/// Streaming write handle. Dropping without `close` may leave a partial file.
pub trait WriteHandle: Send {
    fn write_all(&mut self, buf: &[u8]) -> Result<()>;
    /// Flush and close. Must be called to finalize the file.
    fn close(self: Box<Self>) -> Result<()>;
}

/// All paths are slash-separated and relative to the peer root ("" is the root
/// itself). Implementations must be safe to call concurrently from many
/// threads: the sync engine shares one `Arc<dyn Transport>` per peer.
pub trait Transport: Send + Sync {
    /// List immediate children of a directory.
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>>;
    /// Metadata for one path, or NotFound (also NotFound for symlinks/special files).
    fn stat(&self, path: &str) -> Result<Entry>;
    fn open_read(&self, path: &str) -> Result<Box<dyn ReadHandle>>;
    /// Create the file and any missing parent directories.
    fn open_write(&self, path: &str) -> Result<Box<dyn WriteHandle>>;
    /// Same-filesystem rename. `dst` must not already exist.
    fn rename(&self, src: &str, dst: &str) -> Result<()>;
    fn delete_file(&self, path: &str) -> Result<()>;
    /// Create a directory and any missing parents. Succeeds if it already exists.
    fn create_dir(&self, path: &str) -> Result<()>;
    /// Remove an empty directory.
    fn delete_dir(&self, path: &str) -> Result<()>;
    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<()>;
}

/// Join a relative slash path onto another (either may be empty).
pub fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if name.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{name}")
    }
}
