use std::path::{Path, PathBuf};
use std::io::{Read, Write};
use std::fs::{self, File};
use ssh2::{Session, Sftp};
use std::net::TcpStream;

use crate::timestamp;

/// Filesystem abstraction for both local and SFTP operations.
pub trait FileSystem: Send + Sync {
    fn stat(&self, path: &str) -> Option<FileStat>;
    fn list_dir(&self, path: &str) -> Vec<DirEntry>;
    fn read_file(&self, path: &str) -> Option<Vec<u8>>;
    fn write_file(&self, path: &str, data: &[u8]) -> bool;
    fn rename(&self, from: &str, to: &str) -> bool;
    fn delete_file(&self, path: &str) -> bool;
    fn delete_dir(&self, path: &str) -> bool;
    fn create_dir(&self, path: &str) -> bool;
    fn create_dir_all(&self, path: &str) -> bool;
    fn is_dir_empty(&self, path: &str) -> bool;
    fn set_mtime(&self, path: &str, mtime: &str) -> bool;
    fn clear_read_only(&self, path: &str) -> bool;
}

#[derive(Debug, Clone)]
pub struct FileStat {
    pub mod_time: Option<String>,
    pub byte_size: i64,
    pub is_dir: bool,
    pub is_symlink: bool,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub mod_time: Option<String>,
    pub byte_size: i64,
}

/// Local filesystem implementation.
pub struct LocalFileSystem {
    root: PathBuf,
}

impl LocalFileSystem {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn full_path(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }
}

impl FileSystem for LocalFileSystem {
    fn stat(&self, path: &str) -> Option<FileStat> {
        let full = self.full_path(path);
        let metadata = full.symlink_metadata().ok()?;

        let is_symlink = metadata.file_type().is_symlink();
        let is_dir = metadata.is_dir();
        let byte_size = if is_dir { -1 } else { metadata.len() as i64 };
        let mod_time = if is_dir {
            None
        } else {
            metadata.modified().ok().map(|t| timestamp::from_system_time(t))
        };

        Some(FileStat {
            mod_time,
            byte_size,
            is_dir,
            is_symlink,
        })
    }

    fn list_dir(&self, path: &str) -> Vec<DirEntry> {
        let full = self.full_path(path);
        let mut entries = Vec::new();

        if let Ok(iter) = fs::read_dir(&full) {
            for entry in iter.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(metadata) = entry.metadata() {
                    let is_symlink = entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false);
                    let is_dir = metadata.is_dir();
                    let byte_size = if is_dir { -1 } else { metadata.len() as i64 };
                    let mod_time = if is_dir {
                        None
                    } else {
                        metadata.modified().ok().map(|t| timestamp::from_system_time(t))
                    };

                    entries.push(DirEntry {
                        name,
                        is_dir,
                        is_symlink,
                        mod_time,
                        byte_size,
                    });
                }
            }
        }

        entries
    }

    fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        let full = self.full_path(path);
        fs::read(&full).ok()
    }

    fn write_file(&self, path: &str, data: &[u8]) -> bool {
        let full = self.full_path(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&full, data).is_ok()
    }

    fn rename(&self, from: &str, to: &str) -> bool {
        let from_full = self.full_path(from);
        let to_full = self.full_path(to);
        if let Some(parent) = to_full.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::rename(&from_full, &to_full).is_ok()
    }

    fn delete_file(&self, path: &str) -> bool {
        let full = self.full_path(path);
        fs::remove_file(&full).is_ok()
    }

    fn delete_dir(&self, path: &str) -> bool {
        let full = self.full_path(path);
        fs::remove_dir(&full).is_ok()
    }

    fn create_dir(&self, path: &str) -> bool {
        let full = self.full_path(path);
        fs::create_dir(&full).is_ok()
    }

    fn create_dir_all(&self, path: &str) -> bool {
        let full = self.full_path(path);
        fs::create_dir_all(&full).is_ok()
    }

    fn is_dir_empty(&self, path: &str) -> bool {
        let full = self.full_path(path);
        fs::read_dir(&full).map(|mut d| d.next().is_none()).unwrap_or(true)
    }

    fn set_mtime(&self, path: &str, mtime: &str) -> bool {
        let full = self.full_path(path);
        if let Some(dt) = timestamp::parse_timestamp(mtime) {
            let secs = dt.timestamp();
            let system_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
            let _ = filetime::set_file_mtime(&full, filetime::FileTime::from_system_time(system_time));
            true
        } else {
            false
        }
    }

    fn clear_read_only(&self, path: &str) -> bool {
        let full = self.full_path(path);
        if let Ok(mut perms) = fs::metadata(&full).map(|m| m.permissions()) {
            perms.set_readonly(false);
            fs::set_permissions(&full, perms).is_ok()
        } else {
            false
        }
    }
}

/// SFTP filesystem implementation.
pub struct SftpFileSystem {
    sftp: Sftp,
    root: String,
}

impl SftpFileSystem {
    pub fn connect(url: &str, timeout_secs: u32) -> Result<Self, String> {
        let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;

        if parsed.scheme() != "sftp" {
            return Err("URL must have sftp:// scheme".to_string());
        }

        let host = parsed.host_str().ok_or("No host in URL")?;
        let port = parsed.port().unwrap_or(22);
        let user = parsed.username();
        let password = parsed.password().map(|p| {
            percent_encoding::percent_decode_str(p).decode_utf8_lossy().to_string()
        });
        let path = parsed.path().to_string();

        // Connect
        let addr = format!("{}:{}", host, port);
        let stream = TcpStream::connect_timeout(
            &addr.parse().map_err(|_| "Invalid address")?,
            std::time::Duration::from_secs(timeout_secs as u64),
        ).map_err(|e| e.to_string())?;

        let mut session = Session::new().map_err(|e| e.to_string())?;
        session.set_tcp_stream(stream);
        session.handshake().map_err(|e| e.to_string())?;

        // Authenticate
        let authenticated = if let Some(pwd) = password {
            session.userauth_password(user, &pwd).is_ok()
        } else {
            // Try SSH agent
            if let Ok(mut agent) = session.agent() {
                if agent.connect().is_ok() {
                    agent.list_identities().ok();
                    let identities: Vec<_> = agent.identities().unwrap_or_default();
                    identities.iter().any(|id| agent.userauth(user, id).is_ok())
                } else {
                    false
                }
            } else {
                false
            } || {
                // Try key files
                let home = dirs::home_dir().unwrap_or_default();
                let keys = ["id_ed25519", "id_ecdsa", "id_rsa"];
                keys.iter().any(|key| {
                    let key_path = home.join(".ssh").join(key);
                    if key_path.exists() {
                        session.userauth_pubkey_file(user, None, &key_path, None).is_ok()
                    } else {
                        false
                    }
                })
            }
        };

        if !authenticated {
            return Err("Authentication failed".to_string());
        }

        let sftp = session.sftp().map_err(|e| e.to_string())?;

        Ok(Self { sftp, root: path })
    }

    fn full_path(&self, path: &str) -> String {
        if path.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{}", self.root.trim_end_matches('/'), path)
        }
    }
}

impl FileSystem for SftpFileSystem {
    fn stat(&self, path: &str) -> Option<FileStat> {
        let full = self.full_path(path);
        let stat = self.sftp.lstat(Path::new(&full)).ok()?;

        let is_symlink = stat.file_type().is_symlink();
        let is_dir = stat.is_dir();
        let byte_size = if is_dir { -1 } else { stat.size.unwrap_or(0) as i64 };
        let mod_time = if is_dir {
            None
        } else {
            stat.mtime.map(|t| {
                let system_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(t);
                timestamp::from_system_time(system_time)
            })
        };

        Some(FileStat {
            mod_time,
            byte_size,
            is_dir,
            is_symlink,
        })
    }

    fn list_dir(&self, path: &str) -> Vec<DirEntry> {
        let full = self.full_path(path);
        let mut entries = Vec::new();

        if let Ok(items) = self.sftp.readdir(Path::new(&full)) {
            for (path, stat) in items {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let is_symlink = stat.file_type().is_symlink();
                let is_dir = stat.is_dir();
                let byte_size = if is_dir { -1 } else { stat.size.unwrap_or(0) as i64 };
                let mod_time = if is_dir {
                    None
                } else {
                    stat.mtime.map(|t| {
                        let system_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(t);
                        timestamp::from_system_time(system_time)
                    })
                };

                entries.push(DirEntry {
                    name,
                    is_dir,
                    is_symlink,
                    mod_time,
                    byte_size,
                });
            }
        }

        entries
    }

    fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        let full = self.full_path(path);
        let mut file = self.sftp.open(Path::new(&full)).ok()?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).ok()?;
        Some(data)
    }

    fn write_file(&self, path: &str, data: &[u8]) -> bool {
        let full = self.full_path(path);
        // Ensure parent directory exists
        if let Some(parent) = Path::new(&full).parent() {
            let _ = self.sftp.mkdir(parent, 0o755);
        }

        match self.sftp.create(Path::new(&full)) {
            Ok(mut file) => file.write_all(data).is_ok(),
            Err(_) => false,
        }
    }

    fn rename(&self, from: &str, to: &str) -> bool {
        let from_full = self.full_path(from);
        let to_full = self.full_path(to);
        // Ensure parent directory exists
        if let Some(parent) = Path::new(&to_full).parent() {
            let _ = self.sftp.mkdir(parent, 0o755);
        }
        self.sftp.rename(Path::new(&from_full), Path::new(&to_full), None).is_ok()
    }

    fn delete_file(&self, path: &str) -> bool {
        let full = self.full_path(path);
        self.sftp.unlink(Path::new(&full)).is_ok()
    }

    fn delete_dir(&self, path: &str) -> bool {
        let full = self.full_path(path);
        self.sftp.rmdir(Path::new(&full)).is_ok()
    }

    fn create_dir(&self, path: &str) -> bool {
        let full = self.full_path(path);
        self.sftp.mkdir(Path::new(&full), 0o755).is_ok()
    }

    fn create_dir_all(&self, path: &str) -> bool {
        let full = self.full_path(path);
        let parts: Vec<&str> = full.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = String::new();
        for part in parts {
            current.push('/');
            current.push_str(part);
            let _ = self.sftp.mkdir(Path::new(&current), 0o755);
        }
        true
    }

    fn is_dir_empty(&self, path: &str) -> bool {
        let full = self.full_path(path);
        self.sftp.readdir(Path::new(&full))
            .map(|entries| entries.is_empty())
            .unwrap_or(true)
    }

    fn set_mtime(&self, path: &str, mtime: &str) -> bool {
        let full = self.full_path(path);
        if let Some(dt) = timestamp::parse_timestamp(mtime) {
            let secs = dt.timestamp() as u64;
            let stat = ssh2::FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: None,
                atime: Some(secs),
                mtime: Some(secs),
            };
            self.sftp.setstat(Path::new(&full), stat).is_ok()
        } else {
            false
        }
    }

    fn clear_read_only(&self, path: &str) -> bool {
        let full = self.full_path(path);
        if let Ok(stat) = self.sftp.stat(Path::new(&full)) {
            if let Some(perm) = stat.perm {
                let new_stat = ssh2::FileStat {
                    size: None,
                    uid: None,
                    gid: None,
                    perm: Some(perm | 0o200),
                    atime: None,
                    mtime: None,
                };
                return self.sftp.setstat(Path::new(&full), new_stat).is_ok();
            }
        }
        false
    }
}

/// Connect to a peer using the appropriate filesystem.
pub fn connect_to_peer(url: &str, timeout_secs: u32) -> Result<Box<dyn FileSystem>, String> {
    if url.starts_with("file://") {
        let path = url.strip_prefix("file://").unwrap_or("");
        // Handle Windows paths like file:///C:/path
        let path = if path.starts_with('/') && path.chars().nth(2) == Some(':') {
            &path[1..] // Remove leading slash for Windows paths
        } else {
            path
        };
        let path = Path::new(path);
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        Ok(Box::new(LocalFileSystem::new(path)))
    } else if url.starts_with("sftp://") {
        Ok(Box::new(SftpFileSystem::connect(url, timeout_secs)?))
    } else {
        Err(format!("Unsupported URL scheme: {}", url))
    }
}
