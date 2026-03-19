use std::fmt;
use std::io::{Read, Write};

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub mod_time: String, // YYYYMMDDTHHmmss.ffffffZ
    pub byte_size: i64,   // -1 for directories
}

#[derive(Debug, Clone)]
pub struct FileStat {
    pub mod_time: String,
    pub byte_size: i64,
    pub is_dir: bool,
}

#[derive(Debug)]
pub enum PeerError {
    NotFound(String),
    PermissionDenied(String),
    IoError(String),
}

impl fmt::Display for PeerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerError::NotFound(s) => write!(f, "not found: {}", s),
            PeerError::PermissionDenied(s) => write!(f, "permission denied: {}", s),
            PeerError::IoError(s) => write!(f, "I/O error: {}", s),
        }
    }
}

impl std::error::Error for PeerError {}

impl From<std::io::Error> for PeerError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => PeerError::NotFound(e.to_string()),
            std::io::ErrorKind::PermissionDenied => PeerError::PermissionDenied(e.to_string()),
            _ => PeerError::IoError(e.to_string()),
        }
    }
}

/// All sync logic operates through this trait.
/// Both file:// and sftp:// implement it.
pub trait PeerFs: Send + Sync {
    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, PeerError>;
    fn stat(&self, path: &str) -> Result<Option<FileStat>, PeerError>;
    fn read_file_to(&self, path: &str, writer: &mut dyn Write) -> Result<u64, PeerError>;
    fn write_file_from(&self, path: &str, reader: &mut dyn Read) -> Result<(), PeerError>;
    fn rename(&self, src: &str, dst: &str) -> Result<(), PeerError>;
    fn delete_file(&self, path: &str) -> Result<(), PeerError>;
    fn create_dir(&self, path: &str) -> Result<(), PeerError>;
    fn delete_dir(&self, path: &str) -> Result<(), PeerError>;
    fn set_mod_time(&self, path: &str, time: &str) -> Result<(), PeerError>;
}
