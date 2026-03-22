use crate::entry::DirEntry;
use crate::transport::Transport;
use ssh2::{Session, Sftp};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub struct SftpTransport {
    root: String,
    sftp: Mutex<Sftp>,
    _session: Session,
}

impl SftpTransport {
    pub fn connect(
        host: &str,
        port: u16,
        username: Option<&str>,
        password: Option<&str>,
        timeout_secs: u64,
        root: &str,
    ) -> io::Result<Self> {
        let addr = format!("{}:{}", host, port);
        let tcp = TcpStream::connect_timeout(
            &addr
                .parse()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?,
            Duration::from_secs(timeout_secs),
        )?;

        let mut session =
            Session::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        session.set_tcp_stream(tcp);
        session
            .handshake()
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

        let user = username.unwrap_or("root");

        if let Some(pw) = password {
            session
                .userauth_password(user, pw)
                .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e))?;
        } else {
            // Try SSH agent
            let mut agent_ok = false;
            if let Ok(mut agent) = session.agent() {
                if agent.connect().is_ok() && agent.list_identities().is_ok() {
                    if let Ok(identities) = agent.identities() {
                        for identity in &identities {
                            if agent.userauth(user, identity).is_ok() {
                                agent_ok = true;
                                break;
                            }
                        }
                    }
                }
            }

            if !agent_ok && !session.authenticated() {
                // Try key files
                let home = home_dir();
                let key_files = [
                    home.join(".ssh/id_ed25519"),
                    home.join(".ssh/id_ecdsa"),
                    home.join(".ssh/id_rsa"),
                ];

                let mut authed = false;
                for key_file in &key_files {
                    if key_file.exists() {
                        if session
                            .userauth_pubkey_file(user, None, key_file, None)
                            .is_ok()
                        {
                            authed = true;
                            break;
                        }
                    }
                }

                if !authed && !session.authenticated() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "SSH authentication failed: no valid credentials",
                    ));
                }
            }
        }

        let sftp = session
            .sftp()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(Self {
            root: root.to_string(),
            sftp: Mutex::new(sftp),
            _session: session,
        })
    }

    fn remote_path(&self, rel_path: &str) -> PathBuf {
        if rel_path.is_empty() || rel_path == "." {
            PathBuf::from(&self.root)
        } else {
            PathBuf::from(&self.root).join(rel_path)
        }
    }
}

fn home_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
    } else if let Some(profile) = std::env::var_os("USERPROFILE") {
        PathBuf::from(profile)
    } else {
        PathBuf::from(".")
    }
}

impl Transport for SftpTransport {
    fn list_dir(&self, rel_path: &str) -> io::Result<Vec<DirEntry>> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();
        let entries = sftp
            .readdir(&path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut result = Vec::new();
        for (entry_path, stat) in entries {
            let name = entry_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if name == ".kitchensync" || name == "." || name == ".." {
                continue;
            }

            result.push(DirEntry {
                name,
                is_dir: stat.is_dir(),
                mod_time: stat.mtime.unwrap_or(0) as i64,
                size: stat.size.unwrap_or(0),
            });
        }

        Ok(result)
    }

    fn read_file(&self, rel_path: &str) -> io::Result<Vec<u8>> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();
        let mut file = sftp
            .open(&path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn write_file(&self, rel_path: &str, data: &[u8]) -> io::Result<()> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();

        if let Some(parent) = path.parent() {
            let _ = mkdir_recursive(&sftp, parent);
        }

        let mut file = sftp
            .create(&path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        file.write_all(data)?;
        Ok(())
    }

    fn stat(&self, rel_path: &str) -> io::Result<Option<DirEntry>> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();
        match sftp.stat(&path) {
            Ok(stat) => {
                let name = Path::new(rel_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                Ok(Some(DirEntry {
                    name,
                    is_dir: stat.is_dir(),
                    mod_time: stat.mtime.unwrap_or(0) as i64,
                    size: stat.size.unwrap_or(0),
                }))
            }
            Err(_) => Ok(None),
        }
    }

    fn delete_file(&self, rel_path: &str) -> io::Result<()> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();
        sftp.unlink(&path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn remove_dir(&self, rel_path: &str) -> io::Result<()> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();
        sftp.rmdir(&path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn mkdir(&self, rel_path: &str) -> io::Result<()> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();
        mkdir_recursive(&sftp, &path)
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let from_path = self.remote_path(from);
        let to_path = self.remote_path(to);
        let sftp = self.sftp.lock().unwrap();

        if let Some(parent) = to_path.parent() {
            let _ = mkdir_recursive(&sftp, parent);
        }

        sftp.rename(&from_path, &to_path, None)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn set_mod_time(&self, rel_path: &str, mod_time: i64) -> io::Result<()> {
        let path = self.remote_path(rel_path);
        let sftp = self.sftp.lock().unwrap();

        let mut stat = sftp
            .stat(&path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        stat.mtime = Some(mod_time as u64);
        sftp.setstat(&path, stat)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

fn mkdir_recursive(sftp: &Sftp, path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let _ = sftp.mkdir(&current, 0o755);
    }
    Ok(())
}
