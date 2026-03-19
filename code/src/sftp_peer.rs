use ssh2::{Session, Sftp};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::peer::{DirEntry, Peer, PeerError};
use crate::timestamp;

pub struct SftpPeer {
    name: String,
    root: PathBuf,
    session: Session,
    sftp: Mutex<Sftp>,
}

// SAFETY: ssh2 Session/Sftp are not Send by default, but we only access
// sftp through a Mutex, serializing access. The underlying libssh2 is
// thread-safe when accessed from one thread at a time.
unsafe impl Send for SftpPeer {}
unsafe impl Sync for SftpPeer {}

impl SftpPeer {
    pub fn connect(
        name: String,
        host: &str,
        port: u16,
        user: Option<&str>,
        password: Option<&str>,
        root: &Path,
        timeout_secs: u64,
    ) -> Result<Self, String> {
        let addr = format!("{}:{}", host, port);
        let tcp = TcpStream::connect_timeout(
            &addr.parse().map_err(|e| format!("Bad address {}: {}", addr, e))?,
            std::time::Duration::from_secs(timeout_secs),
        )
        .map_err(|e| format!("Connect to {} failed: {}", addr, e))?;

        let mut session = Session::new().map_err(|e| format!("SSH session error: {}", e))?;
        session.set_tcp_stream(tcp);
        session
            .handshake()
            .map_err(|e| format!("SSH handshake failed: {}", e))?;

        // Verify host key against known_hosts
        verify_host_key(&session, host, port)?;

        let username = user.unwrap_or("root");

        // Auth fallback chain
        let mut authenticated = false;

        // 1. Inline password
        if let Some(pw) = password {
            if session.userauth_password(username, pw).is_ok() {
                authenticated = true;
            }
        }

        // 2. SSH agent
        if !authenticated {
            if let Ok(mut agent) = session.agent() {
                if agent.connect().is_ok() {
                    agent.list_identities().ok();
                    let identities: Vec<_> = agent.identities().unwrap_or_default();
                    for identity in &identities {
                        if agent.userauth(username, identity).is_ok() {
                            authenticated = true;
                            break;
                        }
                    }
                }
            }
        }

        // 3-5. Key files
        if !authenticated {
            let home = dirs_home()?;
            let key_files = ["id_ed25519", "id_ecdsa", "id_rsa"];
            for key_file in &key_files {
                let key_path = Path::new(&home).join(".ssh").join(key_file);
                if key_path.exists() {
                    if session
                        .userauth_pubkey_file(username, None, &key_path, None)
                        .is_ok()
                    {
                        authenticated = true;
                        break;
                    }
                }
            }
        }

        if !authenticated {
            return Err(format!("Authentication failed for {}@{}", username, host));
        }

        let sftp = session
            .sftp()
            .map_err(|e| format!("SFTP subsystem error: {}", e))?;

        Ok(Self {
            name,
            root: root.to_path_buf(),
            session,
            sftp: Mutex::new(sftp),
        })
    }

    fn full_path(&self, rel: &str) -> PathBuf {
        if rel.is_empty() || rel == "." {
            self.root.clone()
        } else {
            self.root.join(rel)
        }
    }
}

impl Peer for SftpPeer {
    fn name(&self) -> &str {
        &self.name
    }

    fn root_path(&self) -> &Path {
        &self.root
    }

    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, PeerError> {
        let full = self.full_path(path);
        let sftp = self.sftp.lock().unwrap();
        let entries = sftp
            .readdir(&full)
            .map_err(|e| PeerError::Io(e.to_string()))?;

        let mut result = Vec::new();
        for (path_buf, stat) in entries {
            let ft = stat.file_type();
            // Skip symlinks and special files
            if ft.is_symlink() {
                continue;
            }
            if !ft.is_file() && !ft.is_dir() {
                continue;
            }

            let name = path_buf
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_dir = ft.is_dir();
            let byte_size = if is_dir {
                -1
            } else {
                stat.size.unwrap_or(0) as i64
            };
            let mod_time = mtime_to_timestamp(stat.mtime.unwrap_or(0));

            result.push(DirEntry {
                name,
                is_dir,
                mod_time,
                byte_size,
                is_symlink: false,
            });
        }
        Ok(result)
    }

    fn stat(&self, path: &str) -> Result<DirEntry, PeerError> {
        let full = self.full_path(path);
        let sftp = self.sftp.lock().unwrap();

        // lstat to detect symlinks
        let stat = sftp
            .lstat(&full)
            .map_err(|e| PeerError::Io(e.to_string()))?;

        if stat.file_type().is_symlink() {
            return Err(PeerError::NotFound);
        }

        let is_dir = stat.file_type().is_dir();
        let byte_size = if is_dir {
            -1
        } else {
            stat.size.unwrap_or(0) as i64
        };
        let mod_time = mtime_to_timestamp(stat.mtime.unwrap_or(0));
        let name = full
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        Ok(DirEntry {
            name,
            is_dir,
            mod_time,
            byte_size,
            is_symlink: false,
        })
    }

    fn read_file(&self, path: &str) -> Result<Box<dyn Read + Send>, PeerError> {
        let full = self.full_path(path);
        let sftp = self.sftp.lock().unwrap();
        let file = sftp
            .open(&full)
            .map_err(|e| PeerError::Io(e.to_string()))?;
        // Read entire file into memory for sending across threads
        // (ssh2 File is not Send)
        let mut buf = Vec::new();
        let mut reader = file;
        reader
            .read_to_end(&mut buf)
            .map_err(|e| PeerError::Io(e.to_string()))?;
        Ok(Box::new(std::io::Cursor::new(buf)))
    }

    fn write_file(&self, path: &str, data: &mut dyn Read) -> Result<(), PeerError> {
        let full = self.full_path(path);
        // Ensure parent directories
        if let Some(parent) = full.parent() {
            self.create_dir_recursive(parent)?;
        }
        let sftp = self.sftp.lock().unwrap();
        let mut file = sftp
            .create(&full)
            .map_err(|e| PeerError::Io(e.to_string()))?;
        let mut buf = [0u8; 32768];
        loop {
            let n = data.read(&mut buf).map_err(|e| PeerError::Io(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| PeerError::Io(e.to_string()))?;
        }
        Ok(())
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), PeerError> {
        let src_full = self.full_path(src);
        let dst_full = self.full_path(dst);
        if let Some(parent) = dst_full.parent() {
            self.create_dir_recursive(parent)?;
        }
        let sftp = self.sftp.lock().unwrap();
        sftp.rename(&src_full, &dst_full, None)
            .map_err(|e| PeerError::Io(e.to_string()))?;
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        let sftp = self.sftp.lock().unwrap();
        sftp.unlink(&full)
            .map_err(|e| PeerError::Io(e.to_string()))?;
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        self.create_dir_recursive(&full)
    }

    fn delete_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        let sftp = self.sftp.lock().unwrap();
        sftp.rmdir(&full)
            .map_err(|e| PeerError::Io(e.to_string()))?;
        Ok(())
    }
}

impl SftpPeer {
    fn create_dir_recursive(&self, path: &Path) -> Result<(), PeerError> {
        let sftp = self.sftp.lock().unwrap();
        // Try to create, if parent doesn't exist, create parents
        if sftp.stat(path).is_ok() {
            return Ok(());
        }
        drop(sftp);
        if let Some(parent) = path.parent() {
            self.create_dir_recursive(parent)?;
        }
        let sftp = self.sftp.lock().unwrap();
        // mkdir may fail if it already exists, that's fine
        sftp.mkdir(path, 0o755).ok();
        Ok(())
    }
}

fn mtime_to_timestamp(mtime: u64) -> String {
    let dt = chrono::DateTime::from_timestamp(mtime as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
    timestamp::format_timestamp(dt)
}

fn verify_host_key(session: &Session, host: &str, port: u16) -> Result<(), String> {
    let mut known_hosts = session
        .known_hosts()
        .map_err(|e| format!("Known hosts error: {}", e))?;

    let known_hosts_path = dirs_home()
        .map(|h| PathBuf::from(h).join(".ssh").join("known_hosts"))
        .unwrap_or_default();

    if known_hosts_path.exists() {
        known_hosts
            .read_file(&known_hosts_path, ssh2::KnownHostFileKind::OpenSSH)
            .ok();
    }

    let (key, _key_type) = session
        .host_key()
        .ok_or_else(|| "No host key from server".to_string())?;

    match known_hosts.check_port(host, port, key) {
        ssh2::CheckResult::Match => Ok(()),
        ssh2::CheckResult::Mismatch => {
            Err(format!("Host key mismatch for {}:{}", host, port))
        }
        ssh2::CheckResult::NotFound | ssh2::CheckResult::Failure => {
            Err(format!(
                "Unknown host {}:{}. Add to ~/.ssh/known_hosts first.",
                host, port
            ))
        }
    }
}

fn dirs_home() -> Result<String, String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "Cannot determine home directory".to_string())
}
