use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use ssh2::{Session, Sftp};

use crate::config::PeerUrl;
use crate::peer::{DirEntry, FileStat, PeerError, PeerFs};
use crate::timestamp;

struct SftpInner {
    session: Session,
    sftp: Sftp,
}

pub struct SftpConnection {
    inner: Mutex<SftpInner>,
    root_path: String,
}

// SftpConnection is Send because Mutex<SftpInner> is Send when SftpInner is Send.
// SftpConnection is Sync because Mutex<SftpInner> is Sync when SftpInner is Send.
// Session is Send in ssh2 0.9.x.

impl SftpConnection {
    pub fn connect(url: &PeerUrl, timeout_secs: u64) -> Result<Self, PeerError> {
        let host = url.host.as_deref().unwrap_or("localhost");
        let addr = format!("{}:{}", host, url.port);

        let tcp = TcpStream::connect_timeout(
            &addr
                .parse()
                .map_err(|e| PeerError::IoError(format!("Invalid address {}: {}", addr, e)))?,
            Duration::from_secs(timeout_secs),
        )
        .map_err(|e| PeerError::IoError(format!("Connect to {}: {}", addr, e)))?;

        let mut session =
            Session::new().map_err(|e| PeerError::IoError(format!("SSH session: {}", e)))?;
        session.set_tcp_stream(tcp);
        session.set_timeout(timeout_secs as u32 * 1000);
        session
            .handshake()
            .map_err(|e| PeerError::IoError(format!("SSH handshake with {}: {}", host, e)))?;

        check_known_hosts(&session, host)?;

        let user = url.user.as_deref().unwrap_or("root");
        let mut authenticated = false;

        // 1. Inline password
        if let Some(ref pw) = url.password {
            if session.userauth_password(user, pw).is_ok() {
                authenticated = true;
            }
        }

        // 2. SSH agent
        if !authenticated {
            if session.userauth_agent(user).is_ok() {
                authenticated = true;
            }
        }

        // 3-5. Key files
        if !authenticated {
            let home = dirs_home();
            let key_files = [
                home.join(".ssh/id_ed25519"),
                home.join(".ssh/id_ecdsa"),
                home.join(".ssh/id_rsa"),
            ];
            for key in &key_files {
                if key.exists() {
                    if session.userauth_pubkey_file(user, None, key, None).is_ok() {
                        authenticated = true;
                        break;
                    }
                }
            }
        }

        if !authenticated || !session.authenticated() {
            return Err(PeerError::PermissionDenied(format!(
                "Authentication failed for {}@{}",
                user, host
            )));
        }

        let sftp = session
            .sftp()
            .map_err(|e| PeerError::IoError(format!("SFTP subsystem: {}", e)))?;

        Ok(SftpConnection {
            inner: Mutex::new(SftpInner { session, sftp }),
            root_path: url.path.clone(),
        })
    }

    fn full_path(&self, path: &str) -> String {
        if path.is_empty() || path == "." || path == "./" {
            self.root_path.clone()
        } else {
            format!(
                "{}/{}",
                self.root_path.trim_end_matches('/'),
                path.trim_start_matches('/')
            )
        }
    }
}

fn check_known_hosts(session: &Session, host: &str) -> Result<(), PeerError> {
    let mut known_hosts = session
        .known_hosts()
        .map_err(|e| PeerError::IoError(format!("Known hosts: {}", e)))?;

    let home = dirs_home();
    let kh_path = home.join(".ssh/known_hosts");
    if kh_path.exists() {
        known_hosts
            .read_file(&kh_path, ssh2::KnownHostFileKind::OpenSSH)
            .map_err(|e| PeerError::IoError(format!("Read known_hosts: {}", e)))?;
    }

    let (key, _key_type) = session
        .host_key()
        .ok_or_else(|| PeerError::IoError("No host key".to_string()))?;

    match known_hosts.check(host, key) {
        ssh2::CheckResult::Match => Ok(()),
        ssh2::CheckResult::Mismatch => Err(PeerError::IoError(format!(
            "Host key mismatch for {}",
            host
        ))),
        ssh2::CheckResult::NotFound => Err(PeerError::IoError(format!(
            "Unknown host: {} (not in known_hosts)",
            host
        ))),
        _ => Err(PeerError::IoError(format!(
            "Host key check failed for {}",
            host
        ))),
    }
}

fn dirs_home() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else if let Ok(home) = std::env::var("USERPROFILE") {
        PathBuf::from(home)
    } else {
        PathBuf::from(".")
    }
}

impl PeerFs for SftpConnection {
    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, PeerError> {
        let full = self.full_path(path);
        let inner = self.inner.lock().unwrap();
        let rd = inner
            .sftp
            .readdir(std::path::Path::new(&full))
            .map_err(|e| PeerError::IoError(format!("list_dir {}: {}", full, e)))?;

        let mut entries = Vec::new();
        for (pathbuf, file_stat) in rd {
            let name = pathbuf
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if name == ".kitchensync" || name == ".git" {
                continue;
            }

            let is_dir = file_stat.is_dir();
            let is_file = file_stat.is_file();
            if !is_dir && !is_file {
                continue;
            }

            let mod_time = file_stat
                .mtime
                .map(|t| {
                    let st = std::time::UNIX_EPOCH + Duration::from_secs(t);
                    timestamp::from_system_time(st)
                })
                .unwrap_or_else(|| timestamp::now());

            let byte_size = if is_dir {
                -1
            } else {
                file_stat.size.unwrap_or(0) as i64
            };

            entries.push(DirEntry {
                name,
                is_dir,
                mod_time,
                byte_size,
            });
        }
        Ok(entries)
    }

    fn stat(&self, path: &str) -> Result<Option<FileStat>, PeerError> {
        let full = self.full_path(path);
        let inner = self.inner.lock().unwrap();
        match inner.sftp.stat(std::path::Path::new(&full)) {
            Ok(file_stat) => {
                let mod_time = file_stat
                    .mtime
                    .map(|t| {
                        let st = std::time::UNIX_EPOCH + Duration::from_secs(t);
                        timestamp::from_system_time(st)
                    })
                    .unwrap_or_else(|| timestamp::now());
                Ok(Some(FileStat {
                    mod_time,
                    byte_size: if file_stat.is_dir() {
                        -1
                    } else {
                        file_stat.size.unwrap_or(0) as i64
                    },
                    is_dir: file_stat.is_dir(),
                }))
            }
            Err(e) => {
                // Check for "no such file" — ssh2 error message contains the path
                let msg = e.message().to_lowercase();
                if msg.contains("no such file") || e.code() == ssh2::ErrorCode::SFTP(2) {
                    // -31 = LIBSSH2_ERROR_SFTP_PROTOCOL, often "no such file"
                    Ok(None)
                } else {
                    Err(PeerError::IoError(format!("stat {}: {}", full, e)))
                }
            }
        }
    }

    fn read_file_to(&self, path: &str, writer: &mut dyn Write) -> Result<u64, PeerError> {
        let full = self.full_path(path);
        let inner = self.inner.lock().unwrap();
        let mut file = inner
            .sftp
            .open(std::path::Path::new(&full))
            .map_err(|e| PeerError::IoError(format!("open {}: {}", full, e)))?;
        let mut buf = [0u8; 32768];
        let mut total = 0u64;
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| PeerError::IoError(format!("read {}: {}", full, e)))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| PeerError::IoError(format!("write: {}", e)))?;
            total += n as u64;
        }
        Ok(total)
    }

    fn write_file_from(&self, path: &str, reader: &mut dyn Read) -> Result<(), PeerError> {
        let full = self.full_path(path);
        let inner = self.inner.lock().unwrap();
        // Ensure parent directory exists
        let parent = std::path::Path::new(&full)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        mkdir_p(&inner.sftp, &parent);

        let mut file = inner
            .sftp
            .create(std::path::Path::new(&full))
            .map_err(|e| PeerError::IoError(format!("create {}: {}", full, e)))?;
        let mut buf = [0u8; 32768];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| PeerError::IoError(format!("read: {}", e)))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| PeerError::IoError(format!("write {}: {}", full, e)))?;
        }
        Ok(())
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), PeerError> {
        let src_full = self.full_path(src);
        let dst_full = self.full_path(dst);
        let inner = self.inner.lock().unwrap();
        let parent = std::path::Path::new(&dst_full)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        mkdir_p(&inner.sftp, &parent);
        inner
            .sftp
            .rename(
                std::path::Path::new(&src_full),
                std::path::Path::new(&dst_full),
                Some(ssh2::RenameFlags::OVERWRITE),
            )
            .map_err(|e| {
                PeerError::IoError(format!("rename {} -> {}: {}", src_full, dst_full, e))
            })?;
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        let inner = self.inner.lock().unwrap();
        inner
            .sftp
            .unlink(std::path::Path::new(&full))
            .map_err(|e| PeerError::IoError(format!("delete {}: {}", full, e)))?;
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        let inner = self.inner.lock().unwrap();
        mkdir_p(&inner.sftp, &full);
        Ok(())
    }

    fn delete_dir(&self, path: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        let inner = self.inner.lock().unwrap();
        inner
            .sftp
            .rmdir(std::path::Path::new(&full))
            .map_err(|e| PeerError::IoError(format!("rmdir {}: {}", full, e)))?;
        Ok(())
    }

    fn set_mod_time(&self, path: &str, time: &str) -> Result<(), PeerError> {
        let full = self.full_path(path);
        if let Some(dt) = timestamp::parse(time) {
            let epoch = dt.timestamp() as u64;
            let inner = self.inner.lock().unwrap();
            let cur = inner.sftp.stat(std::path::Path::new(&full)).ok();
            let atime = cur.as_ref().and_then(|s| s.mtime).unwrap_or(epoch);
            let new_stat = ssh2::FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: None,
                atime: Some(atime),
                mtime: Some(epoch),
            };
            inner
                .sftp
                .setstat(std::path::Path::new(&full), new_stat)
                .map_err(|e| PeerError::IoError(format!("set_mod_time {}: {}", full, e)))?;
        }
        Ok(())
    }
}

fn mkdir_p(sftp: &Sftp, path: &str) {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut current = String::new();
    for part in parts {
        current = format!("{}/{}", current, part);
        let _ = sftp.mkdir(std::path::Path::new(&current), 0o755);
    }
}
