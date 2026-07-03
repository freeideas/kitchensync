use crate::api::*;
use peertransportsurface::{
    ConnectedPeerRoot, PeerDirectoryEntry, PeerMetadata, PeerReadChunk, PeerReadHandle,
    PeerTransportError, PeerWriteHandle,
};
use ssh2::{CheckResult, ErrorCode, FileStat, KnownHostFileKind, Session, Sftp};
use std::env;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct SftpTransportImpl {
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
}

struct SftpRoot {
    _session: Mutex<Session>,
    sftp: Mutex<Sftp>,
    root_path: String,
}

const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;

fn map_ssh_error(error: ssh2::Error) -> PeerTransportError {
    match error.code() {
        ErrorCode::SFTP(2) => PeerTransportError::NotFound,
        ErrorCode::SFTP(3) => PeerTransportError::PermissionDenied,
        _ => PeerTransportError::IoError,
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            let mut home = PathBuf::from(drive);
            home.push(path);
            Some(home)
        })
}

fn ssh_path(name: &str) -> Option<PathBuf> {
    Some(home_dir()?.join(".ssh").join(name))
}

fn duration_seconds(seconds: u64) -> Duration {
    Duration::from_secs(seconds.max(1))
}

fn timeout_millis(timeout: Duration) -> u32 {
    timeout.as_millis().min(u32::MAX as u128) as u32
}

fn root(peer: &ConnectedPeerRoot) -> Result<&SftpRoot, PeerTransportError> {
    peer.handle
        .downcast_ref::<SftpRoot>()
        .ok_or(PeerTransportError::IoError)
}

fn remote_path(root_path: &str, path: &str) -> Result<String, PeerTransportError> {
    let mut joined = root_path.trim_end_matches('/').to_string();
    if joined.is_empty() {
        joined.push('/');
    }

    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(PeerTransportError::NotFound);
        }
        if joined != "/" {
            joined.push('/');
        }
        joined.push_str(part);
    }

    Ok(joined)
}

fn parent_remote_path(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let index = trimmed.rfind('/')?;
    if index == 0 {
        Some("/".to_string())
    } else {
        Some(trimmed[..index].to_string())
    }
}

fn is_dir(stat: &FileStat) -> bool {
    matches!(stat.perm.map(|perm| perm & S_IFMT), Some(S_IFDIR))
}

fn is_file(stat: &FileStat) -> bool {
    matches!(stat.perm.map(|perm| perm & S_IFMT), Some(S_IFREG))
}

fn metadata_from(stat: FileStat) -> Result<PeerMetadata, PeerTransportError> {
    let directory = is_dir(&stat);
    if !directory && !is_file(&stat) {
        return Err(PeerTransportError::NotFound);
    }

    Ok(PeerMetadata {
        is_dir: directory,
        mod_time: UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0)),
        byte_size: if directory {
            -1
        } else {
            stat.size.unwrap_or(0).try_into().unwrap_or(i64::MAX)
        },
    })
}

fn create_dir_all(sftp: &Sftp, path: &str) -> Result<(), PeerTransportError> {
    let mut current = if path.starts_with('/') {
        "/".to_string()
    } else {
        String::new()
    };

    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(PeerTransportError::NotFound);
        }
        if current != "/" && !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);

        match sftp.stat(Path::new(&current)) {
            Ok(stat) if is_dir(&stat) => {}
            Ok(_) => return Err(PeerTransportError::NotFound),
            Err(_) => sftp.mkdir(Path::new(&current), 0o755).map_err(map_ssh_error)?,
        }
    }

    Ok(())
}

fn verify_known_host(session: &Session, host: &str, port: u16) -> Result<(), PeerTransportError> {
    let (key, _) = session.host_key().ok_or(PeerTransportError::IoError)?;
    let known_hosts = ssh_path("known_hosts").ok_or(PeerTransportError::IoError)?;
    let mut known = session.known_hosts().map_err(map_ssh_error)?;
    known
        .read_file(&known_hosts, KnownHostFileKind::OpenSSH)
        .map_err(map_ssh_error)?;

    if matches!(known.check_port(host, port, key), CheckResult::Match) {
        Ok(())
    } else {
        Err(PeerTransportError::IoError)
    }
}

fn authenticate_with_agent(session: &Session, user: &str) -> Result<(), PeerTransportError> {
    let mut agent = session.agent().map_err(map_ssh_error)?;
    agent.connect().map_err(map_ssh_error)?;
    agent.list_identities().map_err(map_ssh_error)?;

    for identity in agent.identities().map_err(map_ssh_error)? {
        if agent.userauth(user, &identity).is_ok() && session.authenticated() {
            return Ok(());
        }
    }

    Err(PeerTransportError::IoError)
}

fn authenticate_with_key(session: &Session, user: &str, name: &str) -> Result<(), PeerTransportError> {
    let private_key = ssh_path(name).ok_or(PeerTransportError::IoError)?;
    if !private_key.is_file() {
        return Err(PeerTransportError::IoError);
    }

    let public_key = ssh_path(&format!("{name}.pub")).filter(|path| path.is_file());
    session
        .userauth_pubkey_file(user, public_key.as_deref(), &private_key, None)
        .map_err(map_ssh_error)
}

fn authenticate(session: &Session, request: &SftpConnectionRequest) -> Result<(), PeerTransportError> {
    if let Some(password) = &request.inline_password {
        if session.userauth_password(&request.user, password).is_ok() && session.authenticated() {
            return Ok(());
        }
    }

    if env::var_os("SSH_AUTH_SOCK").is_some()
        && authenticate_with_agent(session, &request.user).is_ok()
        && session.authenticated()
    {
        return Ok(());
    }

    for key in ["id_ed25519", "id_ecdsa", "id_rsa"] {
        if authenticate_with_key(session, &request.user, key).is_ok() && session.authenticated() {
            return Ok(());
        }
    }

    Err(PeerTransportError::PermissionDenied)
}

fn open_session(request: &SftpConnectionRequest) -> Result<Session, PeerTransportError> {
    let connection_timeout = duration_seconds(
        request
            .url_timeout_conn_seconds
            .unwrap_or(request.global_timeout_conn_seconds),
    );
    let idle_timeout = duration_seconds(
        request
            .url_timeout_idle_seconds
            .unwrap_or(request.global_timeout_idle_seconds),
    );
    let address = (request.host.as_str(), request.port)
        .to_socket_addrs()
        .map_err(|_| PeerTransportError::IoError)?
        .next()
        .ok_or(PeerTransportError::IoError)?;
    let tcp = TcpStream::connect_timeout(&address, connection_timeout)
        .map_err(|_| PeerTransportError::IoError)?;
    tcp.set_read_timeout(Some(connection_timeout))
        .map_err(|_| PeerTransportError::IoError)?;
    tcp.set_write_timeout(Some(connection_timeout))
        .map_err(|_| PeerTransportError::IoError)?;

    let mut session = Session::new().map_err(map_ssh_error)?;
    session.set_tcp_stream(tcp);
    session.set_timeout(timeout_millis(connection_timeout));
    session.handshake().map_err(map_ssh_error)?;
    verify_known_host(&session, &request.host, request.port)?;
    authenticate(&session, request)?;
    session.set_timeout(timeout_millis(idle_timeout));
    Ok(session)
}

fn read_handle(handle: &mut PeerReadHandle) -> Result<&mut ssh2::File, PeerTransportError> {
    handle
        .handle
        .downcast_mut::<ssh2::File>()
        .ok_or(PeerTransportError::IoError)
}

fn write_handle(handle: &mut PeerWriteHandle) -> Result<&mut ssh2::File, PeerTransportError> {
    handle
        .handle
        .downcast_mut::<ssh2::File>()
        .ok_or(PeerTransportError::IoError)
}

impl SftpTransport for SftpTransportImpl {
    fn connect(
        &self,
        request: SftpConnectionRequest,
    ) -> Result<ConnectedPeerRoot, PeerTransportError> {
        let _ = &self.peertransportsurface;
        let session = open_session(&request)?;
        let sftp = session.sftp().map_err(map_ssh_error)?;
        if request.create_missing_root {
            create_dir_all(&sftp, &request.remote_root_path)?;
        }
        let stat = sftp
            .stat(Path::new(&request.remote_root_path))
            .map_err(map_ssh_error)?;
        if !is_dir(&stat) {
            return Err(PeerTransportError::NotFound);
        }

        Ok(ConnectedPeerRoot {
            handle: Arc::new(SftpRoot {
                _session: Mutex::new(session),
                sftp: Mutex::new(sftp),
                root_path: request.remote_root_path,
            }),
        })
    }

    fn list_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        let mut entries = Vec::new();

        for (child_path, stat) in sftp.readdir(Path::new(&path)).map_err(map_ssh_error)? {
            if !is_dir(&stat) && !is_file(&stat) {
                continue;
            }
            let child_name = child_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(PeerTransportError::IoError)?
                .to_string();
            if child_name == "." || child_name == ".." {
                continue;
            }
            let directory = is_dir(&stat);
            entries.push(PeerDirectoryEntry {
                child_name,
                is_dir: directory,
                mod_time: UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0)),
                byte_size: if directory {
                    -1
                } else {
                    stat.size.unwrap_or(0).try_into().unwrap_or(i64::MAX)
                },
            });
        }

        Ok(entries)
    }

    fn stat(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerMetadata, PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        metadata_from(sftp.stat(Path::new(&path)).map_err(map_ssh_error)?)
    }

    fn open_read(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerReadHandle, PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        let stat = sftp.stat(Path::new(&path)).map_err(map_ssh_error)?;
        if !is_file(&stat) {
            return Err(PeerTransportError::NotFound);
        }
        Ok(PeerReadHandle {
            handle: Box::new(sftp.open(Path::new(&path)).map_err(map_ssh_error)?),
        })
    }

    fn read(
        &self,
        handle: &mut PeerReadHandle,
        max_bytes: usize,
    ) -> Result<PeerReadChunk, PeerTransportError> {
        let mut bytes = vec![0; max_bytes];
        let count = read_handle(handle)?
            .read(&mut bytes)
            .map_err(|_| PeerTransportError::IoError)?;

        if count == 0 {
            Ok(PeerReadChunk::Eof)
        } else {
            bytes.truncate(count);
            Ok(PeerReadChunk::Bytes(bytes))
        }
    }

    fn close_read(&self, handle: PeerReadHandle) -> Result<(), PeerTransportError> {
        if handle.handle.is::<ssh2::File>() {
            Ok(())
        } else {
            Err(PeerTransportError::IoError)
        }
    }

    fn open_write(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerWriteHandle, PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        if let Some(parent) = parent_remote_path(&path) {
            create_dir_all(&sftp, &parent)?;
        }
        Ok(PeerWriteHandle {
            handle: Box::new(sftp.create(Path::new(&path)).map_err(map_ssh_error)?),
        })
    }

    fn write(
        &self,
        handle: &mut PeerWriteHandle,
        bytes: &[u8],
    ) -> Result<(), PeerTransportError> {
        write_handle(handle)?
            .write_all(bytes)
            .map_err(|_| PeerTransportError::IoError)
    }

    fn close_write(&self, handle: PeerWriteHandle) -> Result<(), PeerTransportError> {
        let mut file = handle
            .handle
            .downcast::<ssh2::File>()
            .map_err(|_| PeerTransportError::IoError)?;
        file.flush().map_err(|_| PeerTransportError::IoError)
    }

    fn rename(
        &self,
        peer: &ConnectedPeerRoot,
        src: &str,
        dst: &str,
    ) -> Result<(), PeerTransportError> {
        let root = root(peer)?;
        let src = remote_path(&root.root_path, src)?;
        let dst = remote_path(&root.root_path, dst)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        match sftp.stat(Path::new(&dst)) {
            Ok(_) => return Err(PeerTransportError::IoError),
            Err(error) if map_ssh_error(error) == PeerTransportError::NotFound => {}
            Err(error) => return Err(map_ssh_error(error)),
        }
        sftp.rename(Path::new(&src), Path::new(&dst), None)
            .map_err(map_ssh_error)
    }

    fn delete_file(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        if !is_file(&sftp.stat(Path::new(&path)).map_err(map_ssh_error)?) {
            return Err(PeerTransportError::NotFound);
        }
        sftp.unlink(Path::new(&path)).map_err(map_ssh_error)
    }

    fn create_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        create_dir_all(&sftp, &path)
    }

    fn delete_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        sftp.rmdir(Path::new(&path)).map_err(map_ssh_error)
    }

    fn set_mod_time(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
        mod_time: SystemTime,
    ) -> Result<(), PeerTransportError> {
        let root = root(peer)?;
        let path = remote_path(&root.root_path, path)?;
        let seconds = mod_time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PeerTransportError::IoError)?
            .as_secs()
            .try_into()
            .map_err(|_| PeerTransportError::IoError)?;
        let sftp = root.sftp.lock().map_err(|_| PeerTransportError::IoError)?;
        metadata_from(sftp.stat(Path::new(&path)).map_err(map_ssh_error)?)?;
        sftp.setstat(
            Path::new(&path),
            FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: None,
                atime: Some(seconds),
                mtime: Some(seconds),
            },
        )
        .map_err(map_ssh_error)
    }
}

pub fn new(
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
) -> std::sync::Arc<dyn SftpTransport> {
    Arc::new(SftpTransportImpl {
        peertransportsurface,
    })
}
