use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use crate::api::*;

impl std::fmt::Debug for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::NotFound => write!(f, "TransportError::NotFound"),
            TransportError::PermissionDenied => write!(f, "TransportError::PermissionDenied"),
            TransportError::Io => write!(f, "TransportError::Io"),
        }
    }
}

enum PeerState {
    Local { root: String },
    Sftp { conn: Arc<dyn transport_sftpbackend::SftpConnection>, root: String },
}

enum ActiveRead {
    Local(transport_localbackend::ReadHandle),
    Sftp {
        conn: Arc<dyn transport_sftpbackend::SftpConnection>,
        handle: transport_sftpbackend::ReadHandle,
    },
}

enum ActiveWrite {
    Local(transport_localbackend::WriteHandle),
    Sftp {
        conn: Arc<dyn transport_sftpbackend::SftpConnection>,
        handle: transport_sftpbackend::WriteHandle,
    },
}

struct SharedState {
    peers: HashMap<u64, PeerState>,
    reads: HashMap<u64, ActiveRead>,
    writes: HashMap<u64, ActiveWrite>,
}

struct TransportImpl {
    local: Arc<dyn transport_localbackend::LocalBackend>,
    sftp: Arc<dyn transport_sftpbackend::SftpBackend>,
    url_normalize: Arc<dyn transport_urlnormalize::UrlNormalize>,
    state: Mutex<SharedState>,
    next_id: AtomicU64,
}

fn map_local(e: transport_localbackend::LocalError) -> TransportError {
    match e {
        transport_localbackend::LocalError::NotFound => TransportError::NotFound,
        transport_localbackend::LocalError::PermissionDenied => TransportError::PermissionDenied,
        transport_localbackend::LocalError::Io => TransportError::Io,
    }
}

fn map_sftp(e: transport_sftpbackend::BackendError) -> TransportError {
    match e {
        transport_sftpbackend::BackendError::NotFound => TransportError::NotFound,
        transport_sftpbackend::BackendError::PermissionDenied => TransportError::PermissionDenied,
        transport_sftpbackend::BackendError::Io => TransportError::Io,
    }
}

fn conv_local_entry(e: transport_localbackend::DirEntry) -> DirEntry {
    DirEntry { name: e.name, is_dir: e.is_dir, mod_time: e.mod_time, byte_size: e.byte_size }
}

fn conv_sftp_entry(e: transport_sftpbackend::DirEntry) -> DirEntry {
    DirEntry { name: e.name, is_dir: e.is_dir, mod_time: e.mod_time, byte_size: e.byte_size }
}

fn conv_local_stat(s: transport_localbackend::Stat) -> Stat {
    Stat { mod_time: s.mod_time, byte_size: s.byte_size, is_dir: s.is_dir }
}

fn conv_sftp_stat(m: transport_sftpbackend::FileMeta) -> Stat {
    Stat { mod_time: m.mod_time, byte_size: m.byte_size, is_dir: m.is_dir }
}

// Build the full absolute path for an SFTP operation given the peer root and a relative path.
fn sftp_path(root: &str, path: &str) -> String {
    if path.is_empty() {
        root.to_string()
    } else {
        format!("{}/{}", root, path)
    }
}

// Extract the path component from a canonical sftp:// URL (everything from the first '/' after the authority).
fn sftp_root_path(canonical_url: &str) -> String {
    let rest = canonical_url.strip_prefix("sftp://").unwrap_or("");
    match rest.find('/') {
        Some(pi) => rest[pi..].to_string(),
        None => "/".to_string(),
    }
}

// Read the timeout-conn query parameter from a raw URL before normalization strips it.
fn extract_timeout_conn(url: &str) -> Option<Duration> {
    let query = url.split_once('?')?.1;
    for param in query.split('&').filter(|s| !s.is_empty()) {
        if let Some(val) = param.strip_prefix("timeout-conn=") {
            if let Ok(n) = val.parse::<u64>() {
                return Some(Duration::from_secs(n));
            }
        }
    }
    None
}

enum PeerRef {
    Local(String),
    Sftp(Arc<dyn transport_sftpbackend::SftpConnection>, String),
}

fn get_peer(state: &SharedState, id: u64) -> Result<PeerRef, TransportError> {
    match state.peers.get(&id) {
        Some(PeerState::Local { root }) => Ok(PeerRef::Local(root.clone())),
        Some(PeerState::Sftp { conn, root }) => Ok(PeerRef::Sftp(conn.clone(), root.clone())),
        None => Err(TransportError::Io),
    }
}

impl Transport for TransportImpl {
    fn normalize_url(&self, url: &str) -> String {
        self.url_normalize.normalize(url)
    }

    fn open_peer(
        &self,
        primary: &str,
        fallbacks: &[String],
        dry_run: bool,
        timeout_conn: Duration,
    ) -> Option<ConnectedPeer> {
        let all: Vec<&str> = std::iter::once(primary)
            .chain(fallbacks.iter().map(String::as_str))
            .collect();

        for url in all {
            let effective_timeout = extract_timeout_conn(url).unwrap_or(timeout_conn);
            let canonical = self.url_normalize.normalize(url);
            if canonical.starts_with("sftp://") {
                let sftp_root = sftp_root_path(&canonical);
                if let Ok(conn) = self.sftp.connect(&canonical, effective_timeout, dry_run) {
                    let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                    self.state.lock().unwrap().peers.insert(id, PeerState::Sftp { conn, root: sftp_root });
                    return Some(ConnectedPeer { handle: PeerHandle(id), winning_url: canonical });
                }
            } else if self.local.open_root(&canonical, dry_run).is_ok() {
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                self.state.lock().unwrap().peers.insert(id, PeerState::Local { root: canonical.clone() });
                return Some(ConnectedPeer { handle: PeerHandle(id), winning_url: canonical });
            }
        }
        None
    }

    fn list_dir(&self, peer: &PeerHandle, path: &str) -> Result<Vec<DirEntry>, TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => self.local.list_dir(&root, path)
                .map(|es| es.into_iter().map(conv_local_entry).collect())
                .map_err(map_local),
            PeerRef::Sftp(conn, root) => conn.list_dir(&sftp_path(&root, path))
                .map(|es| es.into_iter().map(conv_sftp_entry).collect())
                .map_err(map_sftp),
        }
    }

    fn stat(&self, peer: &PeerHandle, path: &str) -> Result<Stat, TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => self.local.stat(&root, path).map(conv_local_stat).map_err(map_local),
            PeerRef::Sftp(conn, root) => conn.stat(&sftp_path(&root, path)).map(conv_sftp_stat).map_err(map_sftp),
        }
    }

    fn open_read(&self, peer: &PeerHandle, path: &str) -> Result<ReadHandle, TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => {
                let lh = self.local.open_read(&root, path).map_err(map_local)?;
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                self.state.lock().unwrap().reads.insert(id, ActiveRead::Local(lh));
                Ok(ReadHandle(id))
            }
            PeerRef::Sftp(conn, root) => {
                let sh = conn.open_read(&sftp_path(&root, path)).map_err(map_sftp)?;
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                self.state.lock().unwrap().reads.insert(id, ActiveRead::Sftp { conn, handle: sh });
                Ok(ReadHandle(id))
            }
        }
    }

    fn read(
        &self,
        handle: &ReadHandle,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let state = self.state.lock().unwrap();
        match state.reads.get(&handle.0) {
            Some(ActiveRead::Local(lh)) => self.local.read(lh, max_bytes).map_err(map_local),
            Some(ActiveRead::Sftp { conn, handle: sh }) => conn.read(sh, max_bytes)
                .map(|b| if b.is_empty() { None } else { Some(b) })
                .map_err(map_sftp),
            None => Err(TransportError::Io),
        }
    }

    fn close_read(&self, handle: ReadHandle) -> Result<(), TransportError> {
        let ar = self.state.lock().unwrap().reads.remove(&handle.0);
        match ar {
            Some(ActiveRead::Local(lh)) => self.local.close_read(lh).map_err(map_local),
            Some(ActiveRead::Sftp { conn, handle: sh }) => conn.close_read(sh).map_err(map_sftp),
            None => Err(TransportError::Io),
        }
    }

    fn open_write(&self, peer: &PeerHandle, path: &str) -> Result<WriteHandle, TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => {
                let lh = self.local.open_write(&root, path).map_err(map_local)?;
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                self.state.lock().unwrap().writes.insert(id, ActiveWrite::Local(lh));
                Ok(WriteHandle(id))
            }
            PeerRef::Sftp(conn, root) => {
                let sh = conn.open_write(&sftp_path(&root, path)).map_err(map_sftp)?;
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                self.state.lock().unwrap().writes.insert(id, ActiveWrite::Sftp { conn, handle: sh });
                Ok(WriteHandle(id))
            }
        }
    }

    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), TransportError> {
        let state = self.state.lock().unwrap();
        match state.writes.get(&handle.0) {
            Some(ActiveWrite::Local(lh)) => self.local.write(lh, bytes).map_err(map_local),
            Some(ActiveWrite::Sftp { conn, handle: sh }) => conn.write(sh, bytes).map_err(map_sftp),
            None => Err(TransportError::Io),
        }
    }

    fn close_write(&self, handle: WriteHandle) -> Result<(), TransportError> {
        let aw = self.state.lock().unwrap().writes.remove(&handle.0);
        match aw {
            Some(ActiveWrite::Local(lh)) => self.local.close_write(lh).map_err(map_local),
            Some(ActiveWrite::Sftp { conn, handle: sh }) => conn.close_write(sh).map_err(map_sftp),
            None => Err(TransportError::Io),
        }
    }

    fn create_dir(&self, peer: &PeerHandle, path: &str) -> Result<(), TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => self.local.create_dir(&root, path).map_err(map_local),
            PeerRef::Sftp(conn, root) => conn.create_dir(&sftp_path(&root, path)).map_err(map_sftp),
        }
    }

    fn rename(&self, peer: &PeerHandle, src: &str, dst: &str) -> Result<(), TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => self.local.rename(&root, src, dst).map_err(map_local),
            PeerRef::Sftp(conn, root) => conn.rename(&sftp_path(&root, src), &sftp_path(&root, dst)).map_err(map_sftp),
        }
    }

    fn delete_file(&self, peer: &PeerHandle, path: &str) -> Result<(), TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => self.local.delete_file(&root, path).map_err(map_local),
            PeerRef::Sftp(conn, root) => conn.delete_file(&sftp_path(&root, path)).map_err(map_sftp),
        }
    }

    fn delete_dir(&self, peer: &PeerHandle, path: &str) -> Result<(), TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => self.local.delete_dir(&root, path).map_err(map_local),
            PeerRef::Sftp(conn, root) => conn.delete_dir(&sftp_path(&root, path)).map_err(map_sftp),
        }
    }

    fn set_mod_time(
        &self,
        peer: &PeerHandle,
        path: &str,
        time: SystemTime,
    ) -> Result<(), TransportError> {
        let p = { let s = self.state.lock().unwrap(); get_peer(&s, peer.0)? };
        match p {
            PeerRef::Local(root) => self.local.set_mod_time(&root, path, time).map_err(map_local),
            PeerRef::Sftp(conn, root) => conn.set_mod_time(&sftp_path(&root, path), time).map_err(map_sftp),
        }
    }
}

pub fn new() -> Arc<dyn Transport> {
    // Transport owns its three private backends and builds them itself; a caller
    // hands it nothing and never names the helper crates (see SPEC.md).
    let local = transport_localbackend::new();
    let sftp = transport_sftpbackend::new();
    let url_normalize = transport_urlnormalize::new();
    Arc::new(TransportImpl {
        local,
        sftp,
        url_normalize,
        state: Mutex::new(SharedState {
            peers: HashMap::new(),
            reads: HashMap::new(),
            writes: HashMap::new(),
        }),
        next_id: AtomicU64::new(0),
    })
}
