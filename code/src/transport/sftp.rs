//! SFTP transport: implements `Transport` over SSH using `russh` +
//! `russh-sftp`. See specs/sync.md, sections "URL Schemes",
//! "Authentication (fallback chain)", "Peer Transports", "Required
//! Operations" and "Error Semantics", and specs/concurrency.md, section
//! "Connection Establishment".
//!
//! The rest of KitchenSync is synchronous threads, while russh is async. One
//! lazily created multi-thread tokio runtime lives in this module; every
//! synchronous trait method drives its async work with `block_on` on that
//! runtime. The engine shares a single `Arc<dyn Transport>` across threads:
//! `RawSftpSession` methods take `&self` and the SFTP protocol multiplexes by
//! request id, so one session serves all threads at once.
//!
//! The transport talks to russh-sftp's `RawSftpSession` (one method per
//! protocol packet) rather than its high-level `SftpSession`, because the
//! high-level API offers no way to send the `posix-rename@openssh.com`
//! extension. That extension is what `rename` uses: some servers (Apple's
//! fskit-backed SFTP stack on macOS 26 serving exFAT, for one) fail the
//! standard `SSH_FXP_RENAME` with a bare "Failure" even when the destination
//! is absent, while posix-rename succeeds. The "destination must not exist"
//! guard in front of the rename keeps the no-overwrite contract regardless of
//! which request is sent.

use crate::config::PeerUrl;
use crate::transport::{
    Entry, ErrorKind, ReadHandle, Result, Transport, TransportError, WriteHandle,
};
use russh::client;
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg};
use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::extensions::HardlinkExtension;
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags, Packet, StatusCode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

/// The OpenSSH rename extension: same two-string payload as hardlink, but
/// with POSIX semantics (atomic replace of an existing destination).
const POSIX_RENAME: &str = "posix-rename@openssh.com";

/// Bytes per READ/WRITE request unless the server advertises other limits via
/// `limits@openssh.com`. 32 KiB is accepted by every known server.
const DEFAULT_CHUNK: usize = 32 * 1024;
/// SFTP channels opened per peer connection (see `SftpTransport::sessions`).
const SFTP_CHANNELS: usize = 4;

/// How many READ/WRITE requests one transport call keeps in flight at once.
/// The engine copies in 1 MiB buffers, so a call is split into chunks and the
/// chunks are pipelined instead of waiting one round trip per chunk.
const IN_FLIGHT: usize = 16;

/// The one tokio runtime used by every SFTP peer in the process.
fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("kitchensync-sftp")
            .build()
            .expect("build tokio runtime for SFTP")
    })
}

/// Map an SFTP-level error onto the shared transport error categories.
/// Only "no such file" and "permission denied" are distinguished; every other
/// status, plus channel and network failures, is an I/O error (specs/sync.md,
/// "Error Semantics").
pub fn map_err(e: SftpError) -> TransportError {
    match &e {
        SftpError::Status(status) => match status.status_code {
            StatusCode::NoSuchFile => TransportError::not_found(e.to_string()),
            StatusCode::PermissionDenied => {
                TransportError::new(ErrorKind::PermissionDenied, e.to_string())
            }
            _ => TransportError::io(e.to_string()),
        },
        _ => TransportError::io(e.to_string()),
    }
}

/// Whether an SFTP error is a status reply with the given code.
fn is_status(e: &SftpError, code: StatusCode) -> bool {
    matches!(e, SftpError::Status(s) if s.status_code == code)
}

// ---------------------------------------------------------------------------
// SSH client handler: host key verification
// ---------------------------------------------------------------------------

/// Verifies the server's host key against a `known_hosts` file. Unknown and
/// changed host keys are both rejected; the reason is stashed in `reason` so
/// `connect` can report something better than russh's generic handshake error.
struct HostKeyCheck {
    host: String,
    port: u16,
    /// `None` means the user's `~/.ssh/known_hosts`.
    known_hosts: Option<PathBuf>,
    reason: Arc<Mutex<Option<String>>>,
}

impl client::Handler for HostKeyCheck {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let pubkey = server_public_key.public_key();
        // Hosts on a non-default port are recorded as `[host]:port`; russh's
        // helpers already apply that rule, so pass the port through.
        let checked = match &self.known_hosts {
            Some(path) => keys::check_known_hosts_path(&self.host, self.port, &pubkey, path),
            None => keys::check_known_hosts(&self.host, self.port, &pubkey),
        };
        let (ok, why) = match checked {
            Ok(true) => (true, None),
            Ok(false) => (
                false,
                Some(format!(
                    "host key for {} not found in known_hosts",
                    self.host
                )),
            ),
            Err(keys::Error::KeyChanged { line }) => (
                false,
                Some(format!(
                    "host key mismatch for {} (known_hosts line {line})",
                    self.host
                )),
            ),
            Err(e) => (
                false,
                Some(format!("cannot check known_hosts for {}: {e}", self.host)),
            ),
        };
        if let Some(why) = why {
            if let Ok(mut slot) = self.reason.lock() {
                *slot = Some(why);
            }
        }
        Ok(ok)
    }
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// A connected SFTP peer: the live SSH session, the SFTP subsystem session,
/// and the absolute remote root path.
pub struct SftpTransport {
    /// Shared with the streaming handles, which may outlive a transport call
    /// and need to close their server-side handle on drop.
    /// A few SFTP channels over the one SSH connection. OpenSSH serves each
    /// channel from its own single-threaded `sftp-server` process, so requests
    /// on one channel wait for each other; spreading them over several lets
    /// concurrent listings and copies overlap. Every operation picks one
    /// channel and stays on it for its whole exchange (a handle belongs to the
    /// channel that opened it).
    sessions: Vec<Arc<RawSftpSession>>,
    next_session: AtomicUsize,
    /// Largest READ and WRITE payloads the server accepts.
    read_len: usize,
    write_len: usize,
    /// Absolute remote root with no trailing slash; empty string means `/`.
    root: String,
    /// Held only to keep the SSH session alive; dropping it closes the
    /// connection. Behind a mutex so the transport is `Sync`.
    _ssh: Mutex<client::Handle<HostKeyCheck>>,
}

/// Connect to an SFTP peer using the user's `~/.ssh/known_hosts`.
///
/// `timeout_conn_secs` bounds the TCP connect plus SSH handshake, and each
/// later setup step (authentication, opening the SFTP subsystem, preparing the
/// root). `timeout_idle_secs` is the SFTP idle keep-alive TTL, see
/// [`connect_with_known_hosts`]. With `create_root`, a missing root directory
/// and any missing parents are created; without it, a missing root is a
/// `NotFound` error.
pub fn connect(
    url: &PeerUrl,
    timeout_conn_secs: u64,
    timeout_idle_secs: u64,
    create_root: bool,
) -> Result<SftpTransport> {
    connect_with_known_hosts(url, timeout_conn_secs, timeout_idle_secs, create_root, None)
}

/// Same as [`connect`], but with an explicit `known_hosts` file. `None` means
/// the default `~/.ssh/known_hosts`. The explicit form exists so tests can
/// verify a throwaway server's host key without touching the user's file.
///
/// Keep-alive: `timeout_idle_secs` becomes russh's `keepalive_interval` with
/// `keepalive_max = 3`, i.e. after that many seconds without a word from the
/// server the client sends a keep-alive, and after three unanswered ones the
/// connection is dropped (pending and later operations then fail as I/O
/// errors). The same value is also used as russh-sftp's per-request response
/// deadline, raised to at least 60 seconds so that a slow but healthy transfer
/// is not cut short. A `timeout_idle_secs` of 0 disables the keep-alive.
pub fn connect_with_known_hosts(
    url: &PeerUrl,
    timeout_conn_secs: u64,
    timeout_idle_secs: u64,
    create_root: bool,
    known_hosts_path: Option<&Path>,
) -> Result<SftpTransport> {
    let conn_timeout = Duration::from_secs(timeout_conn_secs.max(1));
    let user = match &url.user {
        Some(u) if !u.is_empty() => u.clone(),
        _ => default_user().ok_or_else(|| {
            TransportError::new(
                ErrorKind::PermissionDenied,
                format!(
                    "no user in {} and no USER/LOGNAME/USERNAME in the environment",
                    url.normalized
                ),
            )
        })?,
    };
    let host = url.host.clone();
    let port = url.port;
    let addr = format!("{host}:{port}");
    let password = url.password.clone();
    let root = normalize_root(&url.path);
    let known_hosts = known_hosts_path.map(|p| p.to_path_buf());

    let mut config = client::Config::default();
    if timeout_idle_secs > 0 {
        config.keepalive_interval = Some(Duration::from_secs(timeout_idle_secs));
        config.keepalive_max = 3;
    }
    config.nodelay = true;
    let config = Arc::new(config);

    let reason = Arc::new(Mutex::new(None::<String>));
    let handler = HostKeyCheck {
        host: host.clone(),
        port,
        known_hosts,
        reason: Arc::clone(&reason),
    };

    runtime().block_on(async move {
        // 1. TCP connect + SSH handshake, including host key verification.
        let connecting = client::connect(config, addr.clone(), handler);
        let mut ssh = match tokio::time::timeout(conn_timeout, connecting).await {
            Err(_) => {
                return Err(TransportError::io(format!(
                    "connecting to {addr} timed out after {}s",
                    conn_timeout.as_secs()
                )));
            }
            Ok(Err(e)) => {
                // A rejected host key surfaces as a generic handshake failure;
                // prefer the specific reason the handler recorded.
                let recorded = reason.lock().ok().and_then(|mut r| r.take());
                return Err(TransportError::io(match recorded {
                    Some(why) => why,
                    None => format!("cannot connect to {addr}: {e}"),
                }));
            }
            Ok(Ok(ssh)) => ssh,
        };

        // 2. Authentication fallback chain.
        let authed = match tokio::time::timeout(
            conn_timeout,
            authenticate(&mut ssh, &user, password.as_deref()),
        )
        .await
        {
            Err(_) => {
                return Err(TransportError::io(format!(
                    "authentication with {addr} timed out after {}s",
                    conn_timeout.as_secs()
                )));
            }
            Ok(Err(e)) => {
                return Err(TransportError::io(format!(
                    "authentication with {addr} failed: {e}"
                )));
            }
            Ok(Ok(authed)) => authed,
        };
        if !authed {
            return Err(TransportError::new(
                ErrorKind::PermissionDenied,
                format!("authentication failed for {user}@{host}"),
            ));
        }

        // 3. Open the SFTP subsystem on a session channel.
        let (session, read_len, write_len) =
            match tokio::time::timeout(conn_timeout, open_sftp(&ssh)).await {
                Err(_) => {
                    return Err(TransportError::io(format!(
                        "opening the sftp subsystem on {addr} timed out after {}s",
                        conn_timeout.as_secs()
                    )));
                }
                Ok(r) => r?,
            };
        session.set_timeout(timeout_idle_secs.max(60));

        // 4. Prepare the remote root.
        let abs_root = if root.is_empty() { "/" } else { root.as_str() };
        if create_root {
            mkdir_p(&session, abs_root).await?;
        }
        match session.lstat(abs_root.to_string()).await {
            Ok(reply) if reply.attrs.file_type() == FileType::Dir => {}
            Ok(_) => {
                return Err(TransportError::not_found(format!(
                    "peer root is not a directory: {abs_root}"
                )));
            }
            Err(e) if is_status(&e, StatusCode::NoSuchFile) => {
                return Err(TransportError::not_found(format!(
                    "peer root does not exist: {abs_root}"
                )));
            }
            Err(e) => return Err(map_err(e)),
        }

        let mut sessions = vec![Arc::new(session)];
        for _ in 1..SFTP_CHANNELS {
            match open_sftp(&ssh).await {
                Ok((extra, _, _)) => sessions.push(Arc::new(extra)),
                // A server that limits channels still works, just with less overlap.
                Err(_) => break,
            }
        }
        Ok(SftpTransport {
            sessions,
            next_session: AtomicUsize::new(0),
            read_len,
            write_len,
            root,
            _ssh: Mutex::new(ssh),
        })
    })
}

impl SftpTransport {
    /// Associated-function spelling of [`connect`], for callers that prefer
    /// `SftpTransport::connect(...)`.
    pub fn connect(
        url: &PeerUrl,
        timeout_conn_secs: u64,
        timeout_idle_secs: u64,
        create_root: bool,
    ) -> Result<SftpTransport> {
        connect(url, timeout_conn_secs, timeout_idle_secs, create_root)
    }

    /// The absolute remote path for a slash-separated path relative to the
    /// peer root ("" is the root itself).
    fn remote(&self, rel: &str) -> String {
        let rel = rel.trim_matches('/');
        if rel.is_empty() {
            if self.root.is_empty() {
                "/".to_string()
            } else {
                self.root.clone()
            }
        } else if self.root.is_empty() {
            format!("/{rel}")
        } else {
            format!("{}/{}", self.root, rel)
        }
    }
}

/// Trim a peer root to an absolute path with no trailing slash. `/` becomes
/// the empty string, which `remote` treats as the filesystem root.
fn normalize_root(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn default_user() -> Option<String> {
    ["USER", "LOGNAME", "USERNAME"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok())
        .filter(|u| !u.is_empty())
}

/// Seconds since the Unix epoch, clamped to the u32 field SFTP v3 carries.
fn to_epoch_secs(time: SystemTime) -> u32 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

fn mod_time(attrs: &FileAttributes) -> SystemTime {
    match attrs.mtime {
        Some(secs) => UNIX_EPOCH + Duration::from_secs(secs as u64),
        None => UNIX_EPOCH,
    }
}

/// Build an `Entry` from attributes, or `None` for a symlink or special file:
/// those are silently omitted (specs/sync.md, "Required Operations").
fn entry_from_attrs(name: String, attrs: &FileAttributes) -> Option<Entry> {
    match attrs.file_type() {
        FileType::Dir => Some(Entry {
            name,
            is_dir: true,
            mod_time: mod_time(attrs),
            byte_size: -1,
        }),
        FileType::File => Some(Entry {
            name,
            is_dir: false,
            mod_time: mod_time(attrs),
            byte_size: attrs.size.unwrap_or(0) as i64,
        }),
        FileType::Symlink | FileType::Other => None,
    }
}

/// The basename of an absolute remote path ("" for the filesystem root).
fn base_name(abs: &str) -> String {
    abs.rsplit('/').find(|s| !s.is_empty()).unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------
// Connection helpers
// ---------------------------------------------------------------------------

/// Open the SFTP subsystem and complete the protocol handshake. Returns the
/// session plus the READ and WRITE payload sizes to use with it: the server's
/// advertised `limits@openssh.com` values when it has them, else a size every
/// server accepts.
async fn open_sftp(
    ssh: &client::Handle<HostKeyCheck>,
) -> Result<(RawSftpSession, usize, usize)> {
    let channel = ssh
        .channel_open_session()
        .await
        .map_err(|e| TransportError::io(format!("cannot open ssh session channel: {e}")))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| TransportError::io(format!("server refused the sftp subsystem: {e}")))?;
    let mut session = RawSftpSession::new(channel.into_stream());
    let version = session
        .init()
        .await
        .map_err(|e| TransportError::io(format!("cannot start the sftp session: {e}")))?;

    let mut read_len = DEFAULT_CHUNK;
    let mut write_len = DEFAULT_CHUNK;
    if version
        .extensions
        .get(russh_sftp::extensions::LIMITS)
        .is_some_and(|v| v == "1")
    {
        if let Ok(limits) = session.limits().await {
            let clamp = |n: u64| n.clamp(1024, 256 * 1024) as usize;
            if limits.max_read_len > 0 {
                read_len = clamp(limits.max_read_len);
            }
            if limits.max_write_len > 0 {
                write_len = clamp(limits.max_write_len);
            }
            session.set_limits(limits.into());
        }
    }
    Ok((session, read_len, write_len))
}

/// The authentication fallback chain of specs/sync.md: inline password, SSH
/// agent, then `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, `~/.ssh/id_rsa`. Each
/// source that is absent or rejected falls through to the next. Returns
/// whether some source was accepted; `Err` means the connection itself broke.
async fn authenticate(
    ssh: &mut client::Handle<HostKeyCheck>,
    user: &str,
    password: Option<&str>,
) -> std::result::Result<bool, russh::Error> {
    // (a) Inline password from the URL.
    if let Some(pw) = password {
        if ssh.authenticate_password(user, pw).await?.success() {
            return Ok(true);
        }
    }

    // RSA keys need a signature hash the server accepts; SHA-1 (`None`) is
    // refused by most modern servers. Worked out once, on first need.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut rsa_hashes: Option<Vec<Option<HashAlg>>> = None;

    // (b) SSH agent, trying every identity it offers. `SSH_AUTH_SOCK` is a
    // Unix socket; on Windows the chain falls straight through to key files.
    #[cfg(unix)]
    if std::env::var_os("SSH_AUTH_SOCK").is_some() {
        if let Ok(mut agent) = keys::agent::client::AgentClient::connect_env().await {
            let identities = agent.request_identities().await.unwrap_or_default();
            for identity in identities {
                let pubkey = identity.public_key().into_owned();
                let hashes = if pubkey.algorithm().is_rsa() {
                    if rsa_hashes.is_none() {
                        rsa_hashes = Some(best_rsa_hashes(ssh).await);
                    }
                    rsa_hashes.clone().unwrap_or_else(|| vec![None])
                } else {
                    vec![None]
                };
                for hash in hashes {
                    match ssh
                        .authenticate_publickey_with(user, pubkey.clone(), hash, &mut agent)
                        .await
                    {
                        Ok(result) if result.success() => return Ok(true),
                        Ok(_) => {}
                        // A broken agent is not a broken connection: stop
                        // using the agent and fall through to key files.
                        Err(_) => return authenticate_with_key_files(ssh, user, rsa_hashes).await,
                    }
                }
            }
        }
    }

    // (c)-(e) Key files, in order.
    authenticate_with_key_files(ssh, user, rsa_hashes).await
}

async fn authenticate_with_key_files(
    ssh: &mut client::Handle<HostKeyCheck>,
    user: &str,
    mut rsa_hashes: Option<Vec<Option<HashAlg>>>,
) -> std::result::Result<bool, russh::Error> {
    let Some(home) = std::env::home_dir() else {
        return Ok(false);
    };
    for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
        let path = home.join(".ssh").join(name);
        // Missing, encrypted, or unparsable key files are skipped.
        let Ok(key) = keys::load_secret_key(&path, None) else {
            continue;
        };
        let key = Arc::new(key);
        let hashes = if key.algorithm().is_rsa() {
            if rsa_hashes.is_none() {
                rsa_hashes = Some(best_rsa_hashes(ssh).await);
            }
            rsa_hashes.clone().unwrap_or_else(|| vec![None])
        } else {
            vec![None]
        };
        for hash in hashes {
            let candidate = PrivateKeyWithHashAlg::new(Arc::clone(&key), hash);
            if ssh.authenticate_publickey(user, candidate).await?.success() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// RSA signature hashes to try, best first. When the server advertises
/// `server-sig-algs` its choice is used; otherwise try SHA-256 and then the
/// legacy SHA-1 form.
async fn best_rsa_hashes(ssh: &client::Handle<HostKeyCheck>) -> Vec<Option<HashAlg>> {
    match ssh.best_supported_rsa_hash().await {
        Ok(Some(alg)) => vec![alg],
        _ => vec![Some(HashAlg::Sha256), None],
    }
}

/// Create `abs` and any missing parents. Failing to create a component is
/// tolerated when it turns out to already be a directory.
async fn mkdir_p(session: &RawSftpSession, abs: &str) -> Result<()> {
    let mut so_far = String::new();
    for component in abs.split('/').filter(|c| !c.is_empty()) {
        so_far.push('/');
        so_far.push_str(component);
        if let Err(e) = session
            .mkdir(so_far.clone(), FileAttributes::default())
            .await
        {
            match session.lstat(so_far.clone()).await {
                Ok(reply) if reply.attrs.file_type() == FileType::Dir => {}
                _ => return Err(map_err(e)),
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Streaming handles
// ---------------------------------------------------------------------------

/// Close a server-side handle without waiting for the reply. Used when a
/// handle is dropped without an explicit close, from any thread.
fn close_in_background(session: Arc<RawSftpSession>, handle: String) {
    runtime().spawn(async move {
        let _ = session.close(handle).await;
    });
}

struct SftpReadHandle {
    session: Arc<RawSftpSession>,
    handle: Option<String>,
    offset: u64,
    chunk: usize,
    /// Set once the server reported end of file, so later reads return 0
    /// without another round trip.
    eof: bool,
}

impl ReadHandle for SftpReadHandle {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() || self.eof {
            return Ok(0);
        }
        let Some(handle) = self.handle.clone() else {
            return Err(TransportError::io("read on a closed handle"));
        };
        // Split the caller's buffer into chunks and keep several READ
        // requests in flight; the replies are laid back into `buf` in order.
        let chunk = self.chunk.max(1);
        let mut filled = 0usize;
        while filled < buf.len() && !self.eof {
            let mut tasks = Vec::new();
            let mut at = filled;
            while at < buf.len() && tasks.len() < IN_FLIGHT {
                let len = (buf.len() - at).min(chunk);
                let session = Arc::clone(&self.session);
                let handle = handle.clone();
                let offset = self.offset + (at - filled) as u64;
                tasks.push((
                    at,
                    len,
                    runtime().spawn(async move { session.read(handle, offset, len as u32).await }),
                ));
                at += len;
            }
            for (at, want, task) in tasks {
                if self.eof {
                    // A previous chunk hit EOF; drain but ignore the rest.
                    let _ = runtime().block_on(task);
                    continue;
                }
                let reply = runtime()
                    .block_on(task)
                    .map_err(|e| TransportError::io(format!("sftp read task failed: {e}")))?;
                match reply {
                    Ok(data) => {
                        let n = data.data.len().min(want);
                        buf[at..at + n].copy_from_slice(&data.data[..n]);
                        filled = at + n;
                        self.offset += n as u64;
                        if n < want {
                            // Short read: the file ends here.
                            self.eof = true;
                        }
                    }
                    Err(e) if is_status(&e, StatusCode::Eof) => {
                        self.eof = true;
                    }
                    Err(e) => return Err(map_err(e)),
                }
            }
        }
        Ok(filled)
    }
}

impl Drop for SftpReadHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            close_in_background(Arc::clone(&self.session), handle);
        }
    }
}

struct SftpWriteHandle {
    session: Arc<RawSftpSession>,
    handle: Option<String>,
    offset: u64,
    chunk: usize,
}

impl WriteHandle for SftpWriteHandle {
    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        let Some(handle) = self.handle.clone() else {
            return Err(TransportError::io("write on a closed handle"));
        };
        let chunk = self.chunk.max(1);
        // Pipeline WRITE requests; every reply is checked before returning,
        // so a reported success means the server has taken all the bytes.
        for batch in buf.chunks(chunk * IN_FLIGHT) {
            let mut tasks = Vec::new();
            let mut at = 0usize;
            for piece in batch.chunks(chunk) {
                let session = Arc::clone(&self.session);
                let handle = handle.clone();
                let offset = self.offset + at as u64;
                let data = piece.to_vec();
                tasks.push(runtime().spawn(async move { session.write(handle, offset, data).await }));
                at += piece.len();
            }
            let mut first_err = None;
            for task in tasks {
                let outcome = match runtime().block_on(task) {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(e)) => Err(map_err(e)),
                    Err(e) => Err(TransportError::io(format!("sftp write task failed: {e}"))),
                };
                if let Err(e) = outcome {
                    first_err.get_or_insert(e);
                }
            }
            if let Some(e) = first_err {
                return Err(e);
            }
            self.offset += batch.len() as u64;
        }
        Ok(())
    }

    fn close(mut self: Box<Self>) -> Result<()> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        // Waits for the server's acknowledgement, so a successful close means
        // the file is complete on the server side.
        runtime()
            .block_on(self.session.close(handle))
            .map(|_| ())
            .map_err(map_err)
    }
}

impl Drop for SftpWriteHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            close_in_background(Arc::clone(&self.session), handle);
        }
    }
}

// ---------------------------------------------------------------------------
// Transport implementation
// ---------------------------------------------------------------------------

impl SftpTransport {
    /// The channel for one operation, chosen round-robin.
    fn session(&self) -> Arc<RawSftpSession> {
        let i = self.next_session.fetch_add(1, Ordering::Relaxed) % self.sessions.len();
        Arc::clone(&self.sessions[i])
    }
    /// Send `posix-rename@openssh.com`. `Ok(false)` means the server does not
    /// implement the extension, so the caller may try the standard rename.
    async fn posix_rename(&self, src: &str, dst: &str) -> Result<bool> {
        let session = self.session();
        let payload: Vec<u8> = HardlinkExtension {
            oldpath: src.to_string(),
            newpath: dst.to_string(),
        }
        .try_into()
        .map_err(|e| TransportError::io(format!("cannot encode rename request: {e}")))?;
        match session.extended(POSIX_RENAME, payload).await {
            Ok(Packet::Status(s)) if s.status_code == StatusCode::Ok => Ok(true),
            Ok(Packet::Status(s)) if s.status_code == StatusCode::OpUnsupported => Ok(false),
            Ok(Packet::Status(s)) => Err(map_err(SftpError::Status(s))),
            Ok(_) => Err(TransportError::io("unexpected reply to posix-rename")),
            Err(e) if is_status(&e, StatusCode::OpUnsupported) => Ok(false),
            Err(e) => Err(map_err(e)),
        }
    }
}

impl Transport for SftpTransport {
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>> {
        let session = self.session();
        let dir = self.remote(path);
        runtime().block_on(async {
            let handle = session
                .opendir(dir.clone())
                .await
                .map_err(map_err)?
                .handle;
            let mut out = Vec::new();
            let mut failure = None;
            loop {
                let batch = match session.readdir(handle.as_str()).await {
                    Ok(name) => name.files,
                    Err(e) if is_status(&e, StatusCode::Eof) => break,
                    Err(e) => {
                        failure = Some(map_err(e));
                        break;
                    }
                };
                for file in batch {
                    let name = file.filename;
                    if name == "." || name == ".." {
                        continue;
                    }
                    // Servers normally answer READDIR with lstat-style
                    // attributes. If the type is missing, ask for it
                    // explicitly rather than guess: a symlink must not be
                    // reported as a file.
                    let attrs = if file.attrs.permissions.is_none() {
                        let child = format!("{}/{}", dir.trim_end_matches('/'), name);
                        match session.lstat(child).await {
                            Ok(reply) => reply.attrs,
                            Err(_) => continue,
                        }
                    } else {
                        file.attrs
                    };
                    if let Some(e) = entry_from_attrs(name, &attrs) {
                        out.push(e);
                    }
                }
            }
            let _ = session.close(handle).await;
            match failure {
                Some(e) => Err(e),
                None => Ok(out),
            }
        })
    }

    fn stat(&self, path: &str) -> Result<Entry> {
        let session = self.session();
        let full = self.remote(path);
        runtime().block_on(async {
            // lstat, so a symlink is seen as a symlink and reported missing.
            let reply = session.lstat(full.clone()).await.map_err(map_err)?;
            entry_from_attrs(base_name(&full), &reply.attrs).ok_or_else(|| {
                TransportError::not_found(format!("not a regular file or directory: {path}"))
            })
        })
    }

    fn open_read(&self, path: &str) -> Result<Box<dyn ReadHandle>> {
        let session = self.session();
        let full = self.remote(path);
        let handle = runtime()
            .block_on(
                session
                    .open(full, OpenFlags::READ, FileAttributes::default()),
            )
            .map_err(map_err)?
            .handle;
        Ok(Box::new(SftpReadHandle {
            session: Arc::clone(&session),
            handle: Some(handle),
            offset: 0,
            chunk: self.read_len,
            eof: false,
        }))
    }

    fn open_write(&self, path: &str) -> Result<Box<dyn WriteHandle>> {
        let session = self.session();
        let full = self.remote(path);
        runtime().block_on(async {
            if let Some(parent) = full.rsplit_once('/').map(|(p, _)| p) {
                if !parent.is_empty() {
                    mkdir_p(&session, parent).await?;
                }
            }
            // Write-only, create-if-missing, truncate-if-present.
            let flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE;
            let handle = session
                .open(full, flags, FileAttributes::default())
                .await
                .map_err(map_err)?
                .handle;
            Ok(Box::new(SftpWriteHandle {
                session: Arc::clone(&session),
                handle: Some(handle),
                offset: 0,
                chunk: self.write_len,
            }) as Box<dyn WriteHandle>)
        })
    }

    fn rename(&self, src: &str, dst: &str) -> Result<()> {
        let session = self.session();
        let src_full = self.remote(src);
        let dst_full = self.remote(dst);
        runtime().block_on(async {
            // SFTP servers disagree about rename-over-existing, so refuse it
            // ourselves: the engine relies on rename never clobbering data
            // (specs/sync.md, "Rename Compatibility"). posix-rename below
            // would overwrite, which is why this check must come first.
            if session.lstat(dst_full.clone()).await.is_ok() {
                return Err(TransportError::io(format!("destination exists: {dst}")));
            }
            // Prefer posix-rename: some servers fail the standard rename
            // outright (see the module comment). Fall back to the standard
            // request only when the extension is not implemented.
            if self.posix_rename(&src_full, &dst_full).await? {
                return Ok(());
            }
            session
                .rename(src_full, dst_full)
                .await
                .map(|_| ())
                .map_err(map_err)
        })
    }

    fn delete_file(&self, path: &str) -> Result<()> {
        let session = self.session();
        let full = self.remote(path);
        runtime()
            .block_on(session.remove(full))
            .map(|_| ())
            .map_err(map_err)
    }

    fn create_dir(&self, path: &str) -> Result<()> {
        let session = self.session();
        let full = self.remote(path);
        runtime().block_on(mkdir_p(&session, &full))
    }

    fn delete_dir(&self, path: &str) -> Result<()> {
        let session = self.session();
        let full = self.remote(path);
        runtime()
            .block_on(session.rmdir(full))
            .map(|_| ())
            .map_err(map_err)
    }

    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<()> {
        let session = self.session();
        let full = self.remote(path);
        let secs = to_epoch_secs(time);
        // SFTP v3 carries access and modification time in one flag, and many
        // servers reject a request that sets only one of them.
        let attrs = FileAttributes {
            atime: Some(secs),
            mtime: Some(secs),
            ..FileAttributes::default()
        };
        runtime()
            .block_on(session.setstat(full, attrs))
            .map(|_| ())
            .map_err(map_err)
    }
}

impl Drop for SftpTransport {
    fn drop(&mut self) {
        // Best effort: tell the server we are done before the SSH handle goes.
        // `close_session` is synchronous, so it is safe from any thread.
        for session in &self.sessions {
            let _ = session.close_session();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Scheme;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;

    const SERVER: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../extart/ephemeral-sftp-server.py"
    );

    /// Kills the throwaway server whatever happens to the test.
    struct ChildGuard(Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn have_uv() -> bool {
        Command::new("uv")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Read lines off a child pipe from a helper thread, so a full pipe on one
    /// stream can never block us while we wait on the other.
    fn line_reader<R: std::io::Read + Send + 'static>(pipe: R) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(std::result::Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        rx
    }

    /// The full round trip against a stock server.
    #[test]
    fn sftp_transport_round_trip_against_a_live_server() {
        round_trip(&[]);
    }

    /// Same round trip against a server that fails the standard rename with
    /// a bare "Failure" (as macOS 26's fskit SFTP stack does on exFAT) but
    /// honors posix-rename. The rename steps only pass because the transport
    /// sends posix-rename first.
    #[test]
    fn sftp_transport_renames_where_plain_rename_fails() {
        round_trip(&["--plain-rename-fails"]);
    }

    /// Same round trip against a server without the posix-rename extension:
    /// the transport must fall back to the standard rename.
    #[test]
    fn sftp_transport_falls_back_to_plain_rename() {
        round_trip(&["--no-posix-rename"]);
    }

    /// With both quirks on, no rename request can succeed. This proves the
    /// fake's "plain rename fails" mode is real, so the test above that
    /// passes with only that flag is passing because of posix-rename.
    #[test]
    fn sftp_test_server_can_break_rename_completely() {
        let Some((t, dir)) = start_server(&["--plain-rename-fails", "--no-posix-rename"]) else {
            return;
        };
        let mut w = t.open_write("a.txt").expect("open_write");
        w.write_all(b"a").expect("write");
        w.close().expect("close");
        let err = t.rename("a.txt", "b.txt").expect_err("rename must fail");
        assert_eq!(err.kind, ErrorKind::Io, "got {err}");
        assert!(t.stat("a.txt").is_ok(), "source untouched");
        t.delete_file("a.txt").expect("delete");
        drop(t);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn round_trip(server_flags: &[&str]) {
        let Some((t, dir)) = start_server(server_flags) else {
            return;
        };
        round_trip_checks(&t);
        drop(t);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Start an ephemeral server with the given extra flags and connect to
    /// it. Returns the transport and the temp dir holding its known_hosts, or
    /// `None` when the test must be skipped. The server dies with the
    /// returned transport (its guard is leaked into a thread that outlives
    /// the test only until the process exits).
    fn start_server(server_flags: &[&str]) -> Option<(SftpTransport, PathBuf)> {
        if !have_uv() {
            println!("skipping: `uv` is not on PATH");
            return None;
        }

        let child = Command::new("uv")
            .args(["run", "--script", SERVER, "--password", "pw"])
            .args(server_flags)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the ephemeral sftp server");
        let mut guard = ChildGuard(child);
        let out = line_reader(guard.0.stdout.take().expect("server stdout"));
        let err = line_reader(guard.0.stderr.take().expect("server stderr"));

        // First stdout line is the port. `uv` may need to fetch python and
        // paramiko on a cold cache, so allow plenty of time.
        let port: u16 = out
            .recv_timeout(Duration::from_secs(300))
            .expect("server port line")
            .trim()
            .parse()
            .expect("port number");

        // stderr carries `host key: <type> <base64>`.
        let mut host_key = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            match err.recv_timeout(Duration::from_secs(30)) {
                Ok(line) => {
                    if let Some(rest) = line.strip_prefix("host key: ") {
                        host_key = Some(rest.trim().to_string());
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let host_key = host_key.expect("server host key line");

        // A throwaway known_hosts, so the user's own file is untouched.
        let dir = std::env::temp_dir().join(format!(
            "kitchensync-sftp-test-{}-{}",
            std::process::id(),
            port
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let known_hosts = dir.join("known_hosts");
        {
            let mut f = std::fs::File::create(&known_hosts).expect("known_hosts");
            // Non-default port, so the host is recorded as [host]:port.
            writeln!(f, "[127.0.0.1]:{port} {host_key}").expect("write known_hosts");
        }

        let url = PeerUrl {
            scheme: Scheme::Sftp,
            normalized: format!("sftp://test@127.0.0.1:{port}/"),
            // The server serves a fresh temp directory as `/`.
            path: "/".to_string(),
            user: Some("test".to_string()),
            password: Some("pw".to_string()),
            host: "127.0.0.1".to_string(),
            port,
            timeout_conn: None,
            timeout_idle: None,
        };

        let t = connect_with_known_hosts(&url, 30, 30, false, Some(&known_hosts))
            .expect("connect to the ephemeral server");
        // Keep the server alive for as long as the transport lives.
        SERVERS.lock().unwrap().push(guard);
        Some((t, dir))
    }

    /// Server processes started by tests; killed when the process exits, and
    /// each one also exits on its own once idle or when its parent dies.
    static SERVERS: Mutex<Vec<ChildGuard>> = Mutex::new(Vec::new());

    fn round_trip_checks(t: &SftpTransport) {
        // create_dir makes missing parents too.
        t.create_dir("dir1/dir2").expect("create_dir");
        let dir1 = t.stat("dir1").expect("stat dir1");
        assert!(dir1.is_dir, "dir1 should be a directory");
        assert_eq!(dir1.byte_size, -1, "directories report -1 bytes");

        // Streaming write.
        let body = b"hello sftp world";
        let mut w = t.open_write("dir1/dir2/hello.txt").expect("open_write");
        w.write_all(body).expect("write_all");
        w.close().expect("close write handle");

        // A body larger than one request, written and read in several
        // pipelined chunks, must come back byte for byte.
        let big: Vec<u8> = (0..(3 * DEFAULT_CHUNK * IN_FLIGHT + 12345))
            .map(|i| (i % 251) as u8)
            .collect();
        let mut w = t.open_write("dir1/big.bin").expect("open_write big");
        w.write_all(&big[..100]).expect("write big head");
        w.write_all(&big[100..]).expect("write big rest");
        w.close().expect("close big");
        assert_eq!(t.stat("dir1/big.bin").expect("stat big").byte_size, big.len() as i64);
        let mut r = t.open_read("dir1/big.bin").expect("open_read big");
        let mut got = Vec::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = r.read(&mut buf).expect("read big");
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        assert!(got == big, "big file round trip differs ({} vs {} bytes)", got.len(), big.len());
        drop(r);
        t.delete_file("dir1/big.bin").expect("delete big");

        // list_dir.
        let listing = t.list_dir("dir1/dir2").expect("list_dir");
        assert_eq!(listing.len(), 1, "one entry, got {listing:?}");
        assert_eq!(listing[0].name, "hello.txt");
        assert!(!listing[0].is_dir);
        assert_eq!(listing[0].byte_size, body.len() as i64);

        // stat.
        let st = t.stat("dir1/dir2/hello.txt").expect("stat file");
        assert_eq!(st.name, "hello.txt");
        assert!(!st.is_dir);
        assert_eq!(st.byte_size, body.len() as i64);

        // Streaming read back.
        let mut r = t.open_read("dir1/dir2/hello.txt").expect("open_read");
        let mut got = Vec::new();
        let mut buf = [0u8; 7];
        loop {
            let n = r.read(&mut buf).expect("read");
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(got, body, "round-tripped contents");

        // set_mod_time, verified through stat.
        let when = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        t.set_mod_time("dir1/dir2/hello.txt", when)
            .expect("set_mod_time");
        let st = t.stat("dir1/dir2/hello.txt").expect("stat after set_mod_time");
        assert_eq!(st.mod_time, when, "mod_time should be what we set");

        // rename to a free name.
        t.rename("dir1/dir2/hello.txt", "dir1/dir2/renamed.txt")
            .expect("rename");
        assert!(
            t.stat("dir1/dir2/hello.txt").unwrap_err().is_not_found(),
            "the old name should be gone"
        );
        assert_eq!(
            t.stat("dir1/dir2/renamed.txt").expect("stat renamed").byte_size,
            body.len() as i64
        );

        // rename must refuse to overwrite an existing destination.
        let mut w = t.open_write("dir1/dir2/other.txt").expect("open_write other");
        w.write_all(b"other").expect("write other");
        w.close().expect("close other");
        let clash = t
            .rename("dir1/dir2/renamed.txt", "dir1/dir2/other.txt")
            .expect_err("rename over an existing file must fail");
        assert_eq!(clash.kind, ErrorKind::Io, "got {clash}");
        assert_eq!(
            t.stat("dir1/dir2/other.txt").expect("other still there").byte_size,
            5,
            "the destination must be untouched"
        );

        // Missing paths are NotFound.
        assert!(t.stat("nope/not/here").unwrap_err().is_not_found());

        // Tidy up: delete files, then the directories.
        t.delete_file("dir1/dir2/renamed.txt").expect("delete renamed");
        t.delete_file("dir1/dir2/other.txt").expect("delete other");
        assert!(t.list_dir("dir1/dir2").expect("list empty dir").is_empty());
        t.delete_dir("dir1/dir2").expect("delete_dir dir2");
        t.delete_dir("dir1").expect("delete_dir dir1");
        assert!(t.stat("dir1").unwrap_err().is_not_found());
        assert!(t.list_dir("").expect("list root").is_empty());
    }

    #[test]
    fn sftp_root_paths_are_joined_with_slashes() {
        assert_eq!(normalize_root("/"), "");
        assert_eq!(normalize_root("/srv/photos/"), "/srv/photos");
        assert_eq!(normalize_root("srv/photos"), "/srv/photos");
        assert_eq!(base_name("/srv/photos"), "photos");
        assert_eq!(base_name("/"), "");
    }
}
