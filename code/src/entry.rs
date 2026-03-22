/// Represents a file or directory entry discovered on a peer.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Path relative to the peer root, using forward slashes.
    pub rel_path: String,
    pub is_dir: bool,
    /// Unix timestamp (seconds since epoch).
    pub mod_time: i64,
    /// File size in bytes (0 for directories).
    pub size: u64,
}

/// A single item returned by a directory listing.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub mod_time: i64,
    pub size: u64,
}
