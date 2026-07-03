use std::time::SystemTime;

use peertransportsurface::{
    ConnectedPeerRoot, PeerDirectoryEntry, PeerMetadata, PeerReadChunk, PeerReadHandle,
    PeerTransportError, PeerWriteHandle,
};

pub struct SftpConnectionRequest {
    /// Parsed user name from the normalized `sftp://` candidate URL.
    pub user: String,
    /// Parsed host name from the normalized `sftp://` candidate URL.
    pub host: String,
    /// Parsed port from the normalized `sftp://` candidate URL.
    pub port: u16,
    /// Remote absolute peer root path from the normalized `sftp://` candidate URL.
    pub remote_root_path: String,
    /// Optional password embedded in the candidate URL.
    pub inline_password: Option<String>,
    /// Global `timeout-conn` value used when the URL has no override.
    pub global_timeout_conn_seconds: u64,
    /// Global `timeout-idle` value used when the URL has no override.
    pub global_timeout_idle_seconds: u64,
    /// Per-URL `timeout-conn` override for this candidate only.
    pub url_timeout_conn_seconds: Option<u64>,
    /// Per-URL `timeout-idle` override for this candidate only.
    pub url_timeout_idle_seconds: Option<u64>,
    /// Whether startup may create the missing remote root for a normal run.
    pub create_missing_root: bool,
}

pub trait SftpTransport: Send + Sync {
    /// Connects one normalized `sftp://` candidate URL from its parsed parts.
    /// URL timeout overrides apply only to this candidate. A URL
    /// `timeout-conn` value replaces the global SSH handshake timeout, and a
    /// URL `timeout-idle` value replaces the global SFTP idle keep-alive TTL,
    /// before any network work starts. The SSH handshake is bounded by the
    /// effective connection timeout; if it does not complete in time, this
    /// candidate fails for the current run.
    ///
    /// Host-key verification against `~/.ssh/known_hosts` happens after the
    /// SSH handshake and before authentication. Unknown host keys are rejected.
    /// Authentication is attempted in exactly this order, continuing after
    /// every absent source, unusable source, or rejected credential: inline URL
    /// password, SSH agent from `SSH_AUTH_SOCK`, `~/.ssh/id_ed25519`,
    /// `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`. This preserves Ed25519 key
    /// fallback even when no password, usable agent, or accepted RSA key exists.
    ///
    /// The SFTP subsystem is opened only after host-key verification and
    /// authentication succeed. During normal startup, when
    /// `create_missing_root` is true, the remote root directory and any missing
    /// parents are created through SFTP before success is reported. If the root
    /// cannot be created, this candidate fails for the current run. The returned
    /// root keeps `remote_root_path` as an invariant for all later
    /// root-relative operations, uses SSH/SFTP rather than local filesystem
    /// access, and does not trigger fallback URL selection after it has
    /// connected.
    fn connect(
        &self,
        request: SftpConnectionRequest,
    ) -> Result<ConnectedPeerRoot, PeerTransportError>;

    /// Lists exactly the immediate children of `path` joined under the
    /// connected SFTP root. The operation does not recurse, preserves each
    /// reported child name exactly as the SFTP server reports it, and reports
    /// SFTP connection drops or SFTP timeouts as `I/O error`.
    fn list_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<Vec<PeerDirectoryEntry>, PeerTransportError>;

    /// Returns metadata for a regular file or directory at `path` joined under
    /// the connected SFTP root. Missing paths and entries outside the shared
    /// peer transport surface fail as `not found`; SFTP connection drops and
    /// SFTP timeouts fail as `I/O error`.
    fn stat(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerMetadata, PeerTransportError>;

    /// Opens a regular file at `path` joined under the connected SFTP root for
    /// streaming reads. The path is resolved only against the remote root
    /// stored in the connected handle; later operation failures do not restart
    /// startup fallback selection. SFTP connection drops and SFTP timeouts fail
    /// as `I/O error`.
    fn open_read(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerReadHandle, PeerTransportError>;

    /// Returns the next byte chunk from an open SFTP read handle, in file
    /// order, or EOF. The returned byte chunk must not exceed `max_bytes`. SFTP
    /// connection drops and SFTP timeouts fail as `I/O error`. After the handle
    /// is closed, later reads through that handle are outside this interface's
    /// guarantees.
    fn read(
        &self,
        handle: &mut PeerReadHandle,
        max_bytes: usize,
    ) -> Result<PeerReadChunk, PeerTransportError>;

    /// Closes an open SFTP read handle. SFTP connection drops and SFTP
    /// timeouts fail as `I/O error`. After this operation returns, later reads
    /// through that handle are outside this interface's guarantees.
    fn close_read(&self, handle: PeerReadHandle) -> Result<(), PeerTransportError>;

    /// Opens `path` joined under the connected SFTP root for streaming writes.
    /// Opening a writer creates the target file and any needed parent
    /// directories before bytes are written. The path stays under the remote
    /// root stored in the connected handle. SFTP connection drops and SFTP
    /// timeouts fail as `I/O error`.
    fn open_write(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<PeerWriteHandle, PeerTransportError>;

    /// Writes the supplied bytes to the open SFTP write handle in call order.
    /// SFTP connection drops and SFTP timeouts fail as `I/O error`. After the
    /// handle is closed, later writes through that handle are outside this
    /// interface's guarantees.
    fn write(
        &self,
        handle: &mut PeerWriteHandle,
        bytes: &[u8],
    ) -> Result<(), PeerTransportError>;

    /// Finalizes an open SFTP write handle so later peer reads return the
    /// written bytes, or returns a shared failure category explaining why
    /// finalization failed. SFTP connection drops and SFTP timeouts fail as
    /// `I/O error`. After this operation returns, later writes through that
    /// handle are outside this interface's guarantees.
    fn close_write(&self, handle: PeerWriteHandle) -> Result<(), PeerTransportError>;

    /// Moves `src` to the non-existing `dst` under the same connected SFTP
    /// root. Both paths are joined under the remote root stored in the
    /// connected handle. This operation guarantees only the non-overwrite
    /// rename shape used by the shared peer transport surface; callers must
    /// not depend on replacing an existing destination. SFTP connection drops
    /// and SFTP timeouts fail as `I/O error`.
    fn rename(
        &self,
        peer: &ConnectedPeerRoot,
        src: &str,
        dst: &str,
    ) -> Result<(), PeerTransportError>;

    /// Removes the file at `path` joined under the connected SFTP root. The
    /// operation does not switch roots or retry another candidate URL, and
    /// SFTP connection drops or SFTP timeouts fail as `I/O error`.
    fn delete_file(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Creates the directory at `path` joined under the connected SFTP root
    /// and any needed parent directories. The operation does not switch roots
    /// or retry another candidate URL, and SFTP connection drops or SFTP
    /// timeouts fail as `I/O error`.
    fn create_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Removes the empty directory at `path` joined under the connected SFTP
    /// root. The operation does not switch roots or retry another candidate
    /// URL, and SFTP connection drops or SFTP timeouts fail as `I/O error`.
    fn delete_dir(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
    ) -> Result<(), PeerTransportError>;

    /// Sets the modification time of an existing file or directory at `path`
    /// joined under the connected SFTP root. The operation does not switch
    /// roots or retry another candidate URL, and SFTP connection drops or SFTP
    /// timeouts fail as `I/O error`.
    fn set_mod_time(
        &self,
        peer: &ConnectedPeerRoot,
        path: &str,
        mod_time: SystemTime,
    ) -> Result<(), PeerTransportError>;
}
