use crate::filesystem::{EntryMeta, FsError, PeerFs};
use ssh2::{Session, Sftp};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncRead;

/// SFTP filesystem implementation of PeerFs.
pub struct SftpFs {
    root: String,
    session: Arc<Mutex<Session>>,
}

impl SftpFs {
    pub fn connect(
        host: &str,
        port: u16,
        username: &str,
        password: Option<&str>,
        timeout_secs: u32,
        root_path: &str,
    ) -> Result<Self, FsError> {
        let addr = format!("{}:{}", host, port);
        let tcp = TcpStream::connect_timeout(
            &addr.parse().map_err(|e| FsError::Io(format!("bad address: {}", e)))?,
            Duration::from_secs(timeout_secs as u64),
        )
        .map_err(|e| FsError::Io(format!("connection failed: {}", e)))?;

        let mut session = Session::new()
            .map_err(|e| FsError::Io(format!("session creation failed: {}", e)))?;
        session.set_tcp_stream(tcp);
        session.set_timeout(timeout_secs * 1000);
        session
            .handshake()
            .map_err(|e| FsError::Io(format!("SSH handshake failed: {}", e)))?;

        // Authentication fallback chain
        if let Some(pw) = password {
            session
                .userauth_password(username, pw)
                .map_err(|e| FsError::Io(format!("password auth failed: {}", e)))?;
        } else {
            // Try SSH agent first
            let agent_ok = session.userauth_agent(username).is_ok();
            if !agent_ok {
                // Try key files
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".to_string());
                let key_files = [
                    format!("{}/.ssh/id_ed25519", home),
                    format!("{}/.ssh/id_ecdsa", home),
                    format!("{}/.ssh/id_rsa", home),
                ];
                let mut authed = false;
                for key_file in &key_files {
                    if Path::new(key_file).exists() {
                        if session
                            .userauth_pubkey_file(username, None, Path::new(key_file), None)
                            .is_ok()
                        {
                            authed = true;
                            break;
                        }
                    }
                }
                if !authed {
                    return Err(FsError::Io("all authentication methods failed".to_string()));
                }
            }
        }

        if !session.authenticated() {
            return Err(FsError::Io("authentication failed".to_string()));
        }

        Ok(SftpFs {
            root: root_path.to_string(),
            session: Arc::new(Mutex::new(session)),
        })
    }

    /// Ensure the root directory exists on the remote, creating it and parents if needed.
    pub fn ensure_root_dir(&self) -> Result<(), FsError> {
        let session = self.session.lock().unwrap();
        let sftp = session
            .sftp()
            .map_err(|e| FsError::Io(format!("SFTP subsystem error: {}", e)))?;
        create_dirs_sftp(&sftp, Path::new(&self.root))
    }

    fn full_path(&self, rel: &str) -> String {
        if rel.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{}", self.root.trim_end_matches('/'), rel)
        }
    }

    fn with_sftp<F, R>(&self, f: F) -> Result<R, FsError>
    where
        F: FnOnce(&Sftp) -> Result<R, FsError>,
    {
        let session = self.session.lock().unwrap();
        let sftp = session
            .sftp()
            .map_err(|e| FsError::Io(format!("SFTP subsystem error: {}", e)))?;
        f(&sftp)
    }
}

#[async_trait::async_trait]
impl PeerFs for SftpFs {
    async fn list_dir(&self, path: &str) -> Result<Vec<EntryMeta>, FsError> {
        let full = self.full_path(path);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;
            let entries = sftp
                .readdir(Path::new(&full))
                .map_err(|e| FsError::Io(format!("readdir failed: {}", e)))?;

            let mut result = Vec::new();
            for (path_buf, stat) in entries {
                let name = path_buf
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if name == ".kitchensync" || name == ".git" {
                    continue;
                }

                // Skip symlinks
                if stat.file_type().is_symlink() {
                    continue;
                }

                let is_dir = stat.is_dir();
                let mod_time = stat.mtime.unwrap_or(0) as i64 * 1_000_000;
                let byte_size = if is_dir {
                    -1
                } else {
                    stat.size.unwrap_or(0) as i64
                };

                result.push(EntryMeta {
                    name,
                    is_dir,
                    mod_time,
                    byte_size,
                });
            }
            Ok(result)
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }

    async fn stat(&self, path: &str) -> Result<EntryMeta, FsError> {
        let full = self.full_path(path);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;
            let stat = sftp
                .stat(Path::new(&full))
                .map_err(|e| FsError::NotFound(format!("stat failed: {}", e)))?;
            let name = Path::new(&full)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_dir = stat.is_dir();
            let mod_time = stat.mtime.unwrap_or(0) as i64 * 1_000_000;
            let byte_size = if is_dir {
                -1
            } else {
                stat.size.unwrap_or(0) as i64
            };
            Ok(EntryMeta {
                name,
                is_dir,
                mod_time,
                byte_size,
            })
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }

    async fn read_file(&self, path: &str) -> Result<Pin<Box<dyn AsyncRead + Send>>, FsError> {
        let full = self.full_path(path);
        let session = self.session.clone();

        // Read entire file in blocking task, then wrap as AsyncRead
        let data = tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;
            let mut file = sftp
                .open(Path::new(&full))
                .map_err(|e| FsError::Io(format!("open failed: {}", e)))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| FsError::Io(format!("read failed: {}", e)))?;
            Ok::<Vec<u8>, FsError>(buf)
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))??;

        Ok(Box::pin(std::io::Cursor::new(data)))
    }

    async fn write_file(
        &self,
        path: &str,
        mut data: Pin<Box<dyn AsyncRead + Send>>,
    ) -> Result<(), FsError> {
        // Read all data first
        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut data, &mut buf)
            .await
            .map_err(|e| FsError::Io(e.to_string()))?;

        let full = self.full_path(path);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;

            // Create parent dirs
            let parent = Path::new(&full).parent();
            if let Some(p) = parent {
                create_dirs_sftp(&sftp, p)?;
            }

            let mut file = sftp
                .create(Path::new(&full))
                .map_err(|e| FsError::Io(format!("create failed: {}", e)))?;
            file.write_all(&buf)
                .map_err(|e| FsError::Io(format!("write failed: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }

    async fn rename(&self, src: &str, dst: &str) -> Result<(), FsError> {
        let src_full = self.full_path(src);
        let dst_full = self.full_path(dst);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;

            // Create parent dirs for destination
            if let Some(p) = Path::new(&dst_full).parent() {
                create_dirs_sftp(&sftp, p)?;
            }

            sftp.rename(Path::new(&src_full), Path::new(&dst_full), None)
                .map_err(|e| FsError::Io(format!("rename failed: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }

    async fn delete_file(&self, path: &str) -> Result<(), FsError> {
        let full = self.full_path(path);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;
            sftp.unlink(Path::new(&full))
                .map_err(|e| FsError::Io(format!("delete failed: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }

    async fn create_dir(&self, path: &str) -> Result<(), FsError> {
        let full = self.full_path(path);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;
            create_dirs_sftp(&sftp, Path::new(&full))
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }

    async fn delete_dir(&self, path: &str) -> Result<(), FsError> {
        let full = self.full_path(path);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;
            sftp.rmdir(Path::new(&full))
                .map_err(|e| FsError::Io(format!("rmdir failed: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }

    async fn set_mod_time(&self, path: &str, time_us: i64) -> Result<(), FsError> {
        let full = self.full_path(path);
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let session = session.lock().unwrap();
            let sftp = session
                .sftp()
                .map_err(|e| FsError::Io(format!("SFTP error: {}", e)))?;
            let secs = (time_us / 1_000_000) as u64;
            let mut stat = sftp
                .stat(Path::new(&full))
                .map_err(|e| FsError::Io(format!("stat for setstat failed: {}", e)))?;
            stat.mtime = Some(secs);
            sftp.setstat(Path::new(&full), stat)
                .map_err(|e| FsError::Io(format!("setstat failed: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| FsError::Io(format!("task join error: {}", e)))?
    }
}

fn create_dirs_sftp(sftp: &Sftp, path: &Path) -> Result<(), FsError> {
    let mut components = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if sftp.stat(&current).is_ok() {
            break;
        }
        components.push(current.clone());
        match current.parent() {
            Some(p) if p != current => current = p.to_path_buf(),
            _ => break,
        }
    }
    for dir in components.into_iter().rev() {
        let _ = sftp.mkdir(&dir, 0o755);
    }
    Ok(())
}
