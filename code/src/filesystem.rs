use std::io;
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite};

/// Entry metadata returned by list_dir and stat.
#[derive(Debug, Clone)]
pub struct EntryMeta {
    pub name: String,
    pub is_dir: bool,
    pub mod_time: i64, // microseconds since epoch
    pub byte_size: i64, // -1 for directories
}

/// Peer filesystem trait. Both file:// and sftp:// implement this.
#[async_trait::async_trait]
pub trait PeerFs: Send + Sync {
    /// List immediate children of a directory.
    async fn list_dir(&self, path: &str) -> Result<Vec<EntryMeta>, FsError>;

    /// Stat a single path.
    async fn stat(&self, path: &str) -> Result<EntryMeta, FsError>;

    /// Open file for streaming read.
    async fn read_file(&self, path: &str) -> Result<Pin<Box<dyn AsyncRead + Send>>, FsError>;

    /// Create/overwrite file from stream, creating parent dirs as needed.
    async fn write_file(&self, path: &str, data: Pin<Box<dyn AsyncRead + Send>>) -> Result<(), FsError>;

    /// Same-filesystem rename.
    async fn rename(&self, src: &str, dst: &str) -> Result<(), FsError>;

    /// Remove a file.
    async fn delete_file(&self, path: &str) -> Result<(), FsError>;

    /// Create directory (and parents).
    async fn create_dir(&self, path: &str) -> Result<(), FsError>;

    /// Remove empty directory.
    async fn delete_dir(&self, path: &str) -> Result<(), FsError>;

    /// Set modification time (microseconds since epoch).
    async fn set_mod_time(&self, path: &str, time_us: i64) -> Result<(), FsError>;
}

#[derive(Debug)]
pub enum FsError {
    NotFound(String),
    PermissionDenied(String),
    Io(String),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::NotFound(s) => write!(f, "not found: {}", s),
            FsError::PermissionDenied(s) => write!(f, "permission denied: {}", s),
            FsError::Io(s) => write!(f, "I/O error: {}", s),
        }
    }
}

impl From<io::Error> for FsError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => FsError::NotFound(e.to_string()),
            io::ErrorKind::PermissionDenied => FsError::PermissionDenied(e.to_string()),
            _ => FsError::Io(e.to_string()),
        }
    }
}
