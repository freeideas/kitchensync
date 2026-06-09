//! Public specification for the Transport subproject.
//!
//! Transport is the single uniform filesystem layer every other component uses
//! to touch a peer. It hides whether a peer is a local `file://` directory or a
//! remote `sftp://` server behind one trait, so the rest of the program never
//! branches on scheme and never sees a scheme-specific error. A `file://` peer
//! and an `sftp://` peer with identical contents yield identical results (022.1).

use std::time::{Duration, SystemTime};

/// The only failure categories any Transport operation may report, identical for
/// `file://` and `sftp://` peers (022.17).
///
/// A network failure such as a connection drop or timeout surfaces as `Io`, never
/// as a transport- or scheme-specific error, so callers never match on scheme
/// (022.18). The same category produces the same sync handling regardless of which
/// scheme the peer uses (022.19).
pub enum TransportError {
    /// The path does not exist (022.6), or names a symbolic link or special file
    /// that Transport refuses to treat as a regular entry (022.16).
    NotFound,
    /// The peer rejected the operation for lack of permission.
    PermissionDenied,
    /// Any other failure, including all network failures (022.18).
    Io,
}

/// One immediate child returned by [`Transport::list_dir`] (022.2, 022.3, 022.4).
pub struct DirEntry {
    /// The child's own name, with no path prefix.
    pub name: String,
    /// True for a directory, false for a regular file.
    pub is_dir: bool,
    /// The child's modification time.
    pub mod_time: SystemTime,
    /// The file size in bytes for a regular file, or `-1` for a directory.
    pub byte_size: i64,
}

/// The metadata returned by [`Transport::stat`] for an existing entry (022.5).
pub struct Stat {
    /// The entry's modification time.
    pub mod_time: SystemTime,
    /// The file size in bytes for a regular file, or `-1` for a directory.
    pub byte_size: i64,
    /// True for a directory, false for a regular file.
    pub is_dir: bool,
}

/// An opaque handle to a peer whose winning URL has been selected and bound for
/// the remainder of the run. Every per-peer filesystem operation takes one of
/// these so the call goes through that peer's bound connection.
pub struct PeerHandle(pub u64);

/// An opaque handle to an open streaming read, produced by
/// [`Transport::open_read`] and consumed by [`Transport::read`] /
/// [`Transport::close_read`].
pub struct ReadHandle(pub u64);

/// An opaque handle to an open streaming write, produced by
/// [`Transport::open_write`] and consumed by [`Transport::write`] /
/// [`Transport::close_write`].
pub struct WriteHandle(pub u64);

/// The outcome of selecting and binding a peer's winning URL (005.1 through 005.5).
pub struct ConnectedPeer {
    /// The handle used for every later operation on this peer.
    pub handle: PeerHandle,
    /// The canonical winning URL the peer is now bound to for the whole run.
    pub winning_url: String,
}

/// The uniform per-peer filesystem layer. `Send + Sync` is required so a single
/// `Arc<dyn Transport>` can be shared as the per-run singleton that holds every
/// peer's bound connection state.
pub trait Transport: Send + Sync {
    /// Turn a peer URL into its canonical identity used for comparison and
    /// snapshot lookup.
    ///
    /// The transform lowercases the scheme and hostname, removes the default SFTP
    /// port 22, collapses consecutive slashes, removes a trailing slash, converts
    /// a bare path to a `file://` URL resolved to an absolute path from the current
    /// working directory, percent-decodes unreserved characters, strips
    /// query-string parameters, and inserts the current OS user as the username for
    /// an SFTP URL that omits one (003.1 through 003.10).
    ///
    /// Normalization is deterministic: the same input URL always produces the same
    /// canonical identity, and the worked examples hold exactly (003.11 through
    /// 003.16): `c:/photos/` -> `file:///c:/photos`; `./data` from `/home/user` ->
    /// `file:///home/user/data`; `SFTP://Host:22/path/` -> `sftp://host/path`;
    /// `sftp://host//docs/` -> `sftp://host/docs`;
    /// `sftp://host/path?timeout-conn=60` -> `sftp://host/path`; and
    /// `sftp://host/path` run as OS user `ace` -> `sftp://ace@host/path`.
    fn normalize_url(&self, url: &str) -> String;

    /// Select a peer's winning URL and bind the peer to it for the rest of the run.
    ///
    /// Tries the primary URL first, then each fallback in listed order, taking the
    /// first URL that connects and not trying the rest (005.1 through 005.3). Once a
    /// winning URL is chosen, no other URL for that peer is ever tried again
    /// (005.4, 005.5).
    ///
    /// For each candidate SFTP URL, authentication tries credential sources in this
    /// exact order, skipping any that is absent and falling through on rejection:
    /// the inline URL password (percent-decoded, so `%40` becomes `@` and `%3A`
    /// becomes `:`), then the SSH agent named by `SSH_AUTH_SOCK`, then
    /// `~/.ssh/id_ed25519`, then `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa` (004.1
    /// through 004.7, 004.10). The host key is verified against
    /// `~/.ssh/known_hosts`, accepting a matching host and rejecting one absent from
    /// that file (004.8, 004.9). The handshake is bounded by `timeout_conn`,
    /// overridden by the URL's own `timeout-conn` parameter; on timeout that URL is
    /// abandoned and the next is tried (005.6, 005.7, 005.8).
    ///
    /// A dry run connects exactly as a normal run does -- the same URL ordering,
    /// SFTP authentication, host-key verification, and handshake timeouts -- so
    /// reachability is decided the same way; only peer-side root creation differs
    /// (024.1).
    ///
    /// Root handling depends on `dry_run`: in a normal run a missing root and any
    /// missing parents are created for both schemes, and a URL whose root cannot be
    /// created is treated as failed; in a dry run a missing root is not created and
    /// a URL whose root does not already exist is treated as failed for that run
    /// (005.9 through 005.14, 024.11).
    ///
    /// Returns the bound peer, or `None` when every URL fails, reporting the peer as
    /// unreachable for the run (005.15). Transport returns this verdict only; it
    /// does not decide whether an unreachable peer is skipped, retried, or aborts
    /// the run.
    fn open_peer(
        &self,
        primary: &str,
        fallbacks: &[String],
        dry_run: bool,
        timeout_conn: Duration,
    ) -> Option<ConnectedPeer>;

    /// List a directory's immediate children, each as a [`DirEntry`] (022.2, 022.3,
    /// 022.4).
    ///
    /// Silently omits symbolic links, special files, and any other non-regular
    /// entry (022.15).
    fn list_dir(&self, peer: &PeerHandle, path: &str) -> Result<Vec<DirEntry>, TransportError>;

    /// Return the metadata of an existing regular file or directory (022.5).
    ///
    /// Returns [`TransportError::NotFound`] when the path does not exist (022.6) or
    /// names a symbolic link or special file (022.16).
    fn stat(&self, peer: &PeerHandle, path: &str) -> Result<Stat, TransportError>;

    /// Open a file for streaming read (022.7).
    fn open_read(&self, peer: &PeerHandle, path: &str) -> Result<ReadHandle, TransportError>;

    /// Read the next chunk of at most `max_bytes` bytes, or `None` at end of file
    /// (022.7).
    fn read(
        &self,
        handle: &ReadHandle,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, TransportError>;

    /// Close an open streaming read, releasing its resources (022.7).
    fn close_read(&self, handle: ReadHandle) -> Result<(), TransportError>;

    /// Open a file for streaming write, creating the target file and any missing
    /// parent directories (022.8).
    fn open_write(&self, peer: &PeerHandle, path: &str) -> Result<WriteHandle, TransportError>;

    /// Append the given bytes to an open streaming write (022.8).
    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), TransportError>;

    /// Close an open streaming write, flushing and releasing its resources (022.8).
    fn close_write(&self, handle: WriteHandle) -> Result<(), TransportError>;

    /// Create the directory and any missing parent directories (022.9).
    fn create_dir(&self, peer: &PeerHandle, path: &str) -> Result<(), TransportError>;

    /// Move `src` to `dst`, only when `dst` does not exist (022.10).
    ///
    /// Fails when `dst` already exists; Transport never relies on
    /// rename-over-existing, leaving staged replacement to the callers that need it
    /// (022.11).
    fn rename(&self, peer: &PeerHandle, src: &str, dst: &str) -> Result<(), TransportError>;

    /// Remove a file (022.12).
    fn delete_file(&self, peer: &PeerHandle, path: &str) -> Result<(), TransportError>;

    /// Remove an empty directory (022.13).
    fn delete_dir(&self, peer: &PeerHandle, path: &str) -> Result<(), TransportError>;

    /// Set the modification time of a file or directory (022.14).
    fn set_mod_time(
        &self,
        peer: &PeerHandle,
        path: &str,
        time: SystemTime,
    ) -> Result<(), TransportError>;
}
