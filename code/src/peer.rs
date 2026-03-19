use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub mod_time: String,
    pub byte_size: i64, // -1 for directories
    pub is_symlink: bool,
}

#[derive(Debug)]
pub enum PeerError {
    NotFound,
    PermissionDenied,
    Io(String),
}

impl std::fmt::Display for PeerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerError::NotFound => write!(f, "not found"),
            PeerError::PermissionDenied => write!(f, "permission denied"),
            PeerError::Io(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

pub trait Peer: Send + Sync {
    fn name(&self) -> &str;
    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, PeerError>;
    fn stat(&self, path: &str) -> Result<DirEntry, PeerError>;
    fn read_file(&self, path: &str) -> Result<Box<dyn Read + Send>, PeerError>;
    fn write_file(&self, path: &str, data: &mut dyn Read) -> Result<(), PeerError>;
    fn rename(&self, src: &str, dst: &str) -> Result<(), PeerError>;
    fn delete_file(&self, path: &str) -> Result<(), PeerError>;
    fn create_dir(&self, path: &str) -> Result<(), PeerError>;
    fn delete_dir(&self, path: &str) -> Result<(), PeerError>;
    fn root_path(&self) -> &Path;
}

pub fn connect_peer(
    name: &str,
    urls: &[crate::config::PeerUrl],
    connection_timeout: u64,
) -> Option<Box<dyn Peer>> {
    for url in urls {
        match url.scheme.as_str() {
            "file" => {
                if url.path.exists() || url.path.to_string_lossy() == "." {
                    return Some(Box::new(crate::local_peer::LocalPeer::new(
                        name.to_string(),
                        url.path.clone(),
                    )));
                }
            }
            "sftp" => {
                if let Some(host) = &url.host {
                    match crate::sftp_peer::SftpPeer::connect(
                        name.to_string(),
                        host,
                        url.port.unwrap_or(22),
                        url.user.as_deref(),
                        url.password.as_deref(),
                        &url.path,
                        connection_timeout,
                    ) {
                        Ok(peer) => return Some(Box::new(peer)),
                        Err(_) => continue,
                    }
                }
            }
            _ => {}
        }
    }
    None
}
