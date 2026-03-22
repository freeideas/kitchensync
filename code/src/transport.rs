use crate::entry::DirEntry;
use std::io;

/// Abstraction over local and remote filesystem operations.
/// All paths are relative to the peer's root directory and use forward slashes.
pub trait Transport: Send + Sync {
    fn list_dir(&self, rel_path: &str) -> io::Result<Vec<DirEntry>>;
    fn read_file(&self, rel_path: &str) -> io::Result<Vec<u8>>;
    fn write_file(&self, rel_path: &str, data: &[u8]) -> io::Result<()>;
    fn stat(&self, rel_path: &str) -> io::Result<Option<DirEntry>>;
    fn delete_file(&self, rel_path: &str) -> io::Result<()>;
    fn remove_dir(&self, rel_path: &str) -> io::Result<()>;
    fn mkdir(&self, rel_path: &str) -> io::Result<()>;
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;
    fn set_mod_time(&self, rel_path: &str, mod_time: i64) -> io::Result<()>;
}
