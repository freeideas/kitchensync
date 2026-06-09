use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ssh2::{FileStat, OpenFlags, OpenType, Session};

use crate::api::*;

fn percent_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(b[i + 1]), hex_nibble(b[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn ssh2_err(e: ssh2::Error) -> BackendError {
    match e.code() {
        ssh2::ErrorCode::SFTP(2) | ssh2::ErrorCode::SFTP(9) => BackendError::NotFound,
        ssh2::ErrorCode::SFTP(3) => BackendError::PermissionDenied,
        _ => BackendError::Io,
    }
}

fn io_err(e: std::io::Error) -> BackendError {
    match e.kind() {
        std::io::ErrorKind::NotFound => BackendError::NotFound,
        std::io::ErrorKind::PermissionDenied => BackendError::PermissionDenied,
        _ => BackendError::Io,
    }
}

// Create path and all missing parents, following symlinks on stat checks.
fn sftp_mkdir_p(sftp: &ssh2::Sftp, path: &Path) -> Result<(), BackendError> {
    match sftp.stat(path) {
        Ok(s) if s.is_dir() => return Ok(()),
        Ok(_) => return Err(BackendError::Io),
        Err(_) => {}
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && parent != path {
            sftp_mkdir_p(sftp, parent)?;
        }
    }
    match sftp.mkdir(path, 0o755) {
        Ok(()) => Ok(()),
        Err(_) => match sftp.stat(path) {
            Ok(s) if s.is_dir() => Ok(()),
            _ => Err(BackendError::Io),
        },
    }
}

struct SftpInner {
    read_files: HashMap<u64, Box<ssh2::File>>,
    write_files: HashMap<u64, Box<ssh2::File>>,
    sftp: Box<ssh2::Sftp>,
    session: Box<Session>,
}

unsafe impl Send for SftpInner {}
unsafe impl Sync for SftpInner {}

struct SftpConnectionImpl {
    inner: Mutex<SftpInner>,
    next_id: AtomicU64,
}

impl SftpConnection for SftpConnectionImpl {
    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, BackendError> {
        let inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        let entries = inner.sftp.readdir(Path::new(path)).map_err(ssh2_err)?;
        let mut result = Vec::new();
        for (child_path, stat) in entries {
            let perm = match stat.perm {
                Some(p) => p,
                None => continue,
            };
            let ftype = perm & 0o170000;
            let is_reg = ftype == 0o100000;
            let is_dir = ftype == 0o040000;
            if !is_reg && !is_dir {
                continue;
            }
            let name = match child_path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            let mod_time = UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0));
            let byte_size = if is_dir { -1i64 } else { stat.size.unwrap_or(0) as i64 };
            result.push(DirEntry { name, is_dir, mod_time, byte_size });
        }
        Ok(result)
    }

    fn stat(&self, path: &str) -> Result<FileMeta, BackendError> {
        let inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        let stat = inner.sftp.lstat(Path::new(path)).map_err(ssh2_err)?;
        let perm = stat.perm.unwrap_or(0);
        let ftype = perm & 0o170000;
        let is_reg = ftype == 0o100000;
        let is_dir = ftype == 0o040000;
        if !is_reg && !is_dir {
            return Err(BackendError::NotFound);
        }
        let mod_time = UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0));
        let byte_size = if is_dir { -1i64 } else { stat.size.unwrap_or(0) as i64 };
        Ok(FileMeta { mod_time, byte_size, is_dir })
    }

    fn open_read(&self, path: &str) -> Result<ReadHandle, BackendError> {
        let mut inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        let file = inner.sftp.open_mode(Path::new(path), OpenFlags::READ, 0, OpenType::File)
            .map_err(ssh2_err)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        inner.read_files.insert(id, Box::new(file));
        Ok(ReadHandle(id))
    }

    fn read(&self, handle: &ReadHandle, max_bytes: usize) -> Result<Vec<u8>, BackendError> {
        let mut inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        let file = inner.read_files.get_mut(&handle.0).ok_or(BackendError::Io)?;
        let mut buf = vec![0u8; max_bytes];
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                // libssh2 surfaces SFTP EOF as an error (SSH_FX_EOF = 1) rather
                // than Ok(0), so map it to the expected empty-vec EOF signal.
                let is_eof = e
                    .get_ref()
                    .and_then(|r| r.downcast_ref::<ssh2::Error>())
                    .map_or(false, |se| se.code() == ssh2::ErrorCode::SFTP(1));
                return if is_eof { Ok(vec![]) } else { Err(io_err(e)) };
            }
        };
        buf.truncate(n);
        Ok(buf)
    }

    fn close_read(&self, handle: ReadHandle) -> Result<(), BackendError> {
        let mut inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        inner.read_files.remove(&handle.0);
        Ok(())
    }

    fn open_write(&self, path: &str) -> Result<WriteHandle, BackendError> {
        let mut inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                sftp_mkdir_p(&inner.sftp, parent)?;
            }
        }
        let file = inner.sftp.open_mode(
            Path::new(path),
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
            0o644,
            OpenType::File,
        ).map_err(ssh2_err)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        inner.write_files.insert(id, Box::new(file));
        Ok(WriteHandle(id))
    }

    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), BackendError> {
        let mut inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        let file = inner.write_files.get_mut(&handle.0).ok_or(BackendError::Io)?;
        file.write_all(bytes).map_err(io_err)
    }

    fn close_write(&self, handle: WriteHandle) -> Result<(), BackendError> {
        let mut inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        inner.write_files.remove(&handle.0);
        Ok(())
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), BackendError> {
        let inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        match inner.sftp.lstat(Path::new(dst)) {
            Err(e) if matches!(e.code(), ssh2::ErrorCode::SFTP(2) | ssh2::ErrorCode::SFTP(9)) => {}
            _ => return Err(BackendError::Io),
        }
        inner.sftp.rename(Path::new(src), Path::new(dst), None).map_err(ssh2_err)
    }

    fn create_dir(&self, path: &str) -> Result<(), BackendError> {
        let inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        sftp_mkdir_p(&inner.sftp, Path::new(path))
    }

    fn delete_file(&self, path: &str) -> Result<(), BackendError> {
        let inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        inner.sftp.unlink(Path::new(path)).map_err(ssh2_err)
    }

    fn delete_dir(&self, path: &str) -> Result<(), BackendError> {
        let inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        inner.sftp.rmdir(Path::new(path)).map_err(ssh2_err)
    }

    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<(), BackendError> {
        let secs = time.duration_since(UNIX_EPOCH).map_err(|_| BackendError::Io)?.as_secs();
        let stat = FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: None,
            atime: Some(secs),
            mtime: Some(secs),
        };
        let inner = self.inner.lock().map_err(|_| BackendError::Io)?;
        inner.sftp.setstat(Path::new(path), stat).map_err(ssh2_err)
    }
}

struct SftpBackendImpl;

impl SftpBackend for SftpBackendImpl {
    fn connect(
        &self,
        url: &str,
        default_timeout_conn: Duration,
        dry_run: bool,
    ) -> Result<Arc<dyn SftpConnection>, BackendError> {
        // Parse normalized sftp://[user[:pass]@]host[:port]/path[?timeout-conn=N]
        let rest = url.strip_prefix("sftp://").ok_or(BackendError::Io)?;
        let (authority_path, query) = match rest.find('?') {
            Some(qi) => (&rest[..qi], &rest[qi + 1..]),
            None => (rest, ""),
        };
        let (authority, path) = match authority_path.find('/') {
            Some(pi) => (&authority_path[..pi], &authority_path[pi..]),
            None => (authority_path, "/"),
        };
        let (userinfo, hostport) = match authority.rfind('@') {
            Some(ai) => (Some(&authority[..ai]), &authority[ai + 1..]),
            None => (None, authority),
        };
        let (host, port): (&str, u16) = if hostport.starts_with('[') {
            let end = hostport.find(']').ok_or(BackendError::Io)?;
            let h = &hostport[1..end];
            let p = if hostport.len() > end + 1 && hostport.as_bytes()[end + 1] == b':' {
                hostport[end + 2..].parse().unwrap_or(22)
            } else {
                22
            };
            (h, p)
        } else {
            match hostport.rfind(':') {
                Some(ci) => (&hostport[..ci], hostport[ci + 1..].parse().unwrap_or(22)),
                None => (hostport, 22),
            }
        };
        let (username, raw_password) = match userinfo {
            None => (
                std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default(),
                None,
            ),
            Some(ui) => match ui.find(':') {
                Some(ci) => (percent_decode(&ui[..ci]), Some(ui[ci + 1..].to_string())),
                None => (percent_decode(ui), None),
            },
        };
        let timeout = {
            let mut t = default_timeout_conn;
            for param in query.split('&').filter(|s| !s.is_empty()) {
                if let Some(val) = param.strip_prefix("timeout-conn=") {
                    if let Ok(n) = val.parse::<u64>() {
                        t = Duration::from_secs(n);
                        break;
                    }
                }
            }
            t
        };

        // TCP connect with timeout
        use std::net::ToSocketAddrs;
        let host_port_str = if host.contains(':') {
            format!("[{}]:{}", host, port)
        } else {
            format!("{}:{}", host, port)
        };
        let addr = host_port_str
            .to_socket_addrs()
            .map_err(|_| BackendError::Io)?
            .next()
            .ok_or(BackendError::Io)?;
        let tcp = TcpStream::connect_timeout(&addr, timeout).map_err(|_| BackendError::Io)?;

        // SSH session
        let mut session = Session::new().map_err(|_| BackendError::Io)?;
        session.set_tcp_stream(tcp);
        session.set_timeout(timeout.as_millis().min(u32::MAX as u128) as u32);
        session.handshake().map_err(|_| BackendError::Io)?;

        // Host key verification against ~/.ssh/known_hosts
        {
            let host_key: Vec<u8> = session
                .host_key()
                .ok_or(BackendError::PermissionDenied)
                .map(|(k, _)| k.to_vec())?;
            let mut known_hosts = session.known_hosts().map_err(|_| BackendError::Io)?;
            let kh_path = home_dir().join(".ssh").join("known_hosts");
            if kh_path.exists() {
                known_hosts
                    .read_file(&kh_path, ssh2::KnownHostFileKind::OpenSSH)
                    .map_err(|_| BackendError::Io)?;
            }
            // Empty or absent known_hosts means unknown host -> reject
            match known_hosts.check_port(host, port, &host_key) {
                ssh2::CheckResult::Match => {}
                _ => return Err(BackendError::PermissionDenied),
            }
        }

        // Authentication: URL password, then SSH agent, then key files
        if let Some(ref raw_pass) = raw_password {
            if !session.authenticated() {
                let _ = session.userauth_password(&username, &percent_decode(raw_pass));
            }
        }
        if !session.authenticated() && std::env::var_os("SSH_AUTH_SOCK").is_some() {
            let _ = session.userauth_agent(&username);
        }
        if !session.authenticated() {
            let home = home_dir();
            for key_name in &["id_ed25519", "id_ecdsa", "id_rsa"] {
                if session.authenticated() {
                    break;
                }
                let kp = home.join(".ssh").join(key_name);
                if kp.exists() {
                    let _ = session.userauth_pubkey_file(&username, None, &kp, None);
                }
            }
        }
        if !session.authenticated() {
            return Err(BackendError::PermissionDenied);
        }

        let session_box = Box::new(session);
        let sftp_box = Box::new(session_box.sftp().map_err(|_| BackendError::Io)?);

        // Handle peer root
        if dry_run {
            match sftp_box.stat(Path::new(path)) {
                Ok(s) if s.is_dir() => {}
                _ => return Err(BackendError::Io),
            }
        } else {
            sftp_mkdir_p(&sftp_box, Path::new(path))?;
        }

        let inner = SftpInner {
            read_files: HashMap::new(),
            write_files: HashMap::new(),
            sftp: sftp_box,
            session: session_box,
        };
        Ok(Arc::new(SftpConnectionImpl {
            inner: Mutex::new(inner),
            next_id: AtomicU64::new(1),
        }))
    }
}

pub fn new() -> std::sync::Arc<dyn SftpBackend> {
    Arc::new(SftpBackendImpl)
}
