use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionStartupRequest {
    pub peers: Vec<PeerConnectionPeer>,
    pub global_connection: PeerConnectionGlobalSettings,
    pub run_mode: PeerConnectionRunMode,
    pub local_environment: PeerConnectionLocalEnvironment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionPeer {
    pub identity: String,
    pub role: PeerConnectionPeerRole,
    pub urls: Vec<PeerConnectionUrl>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionPeerRole {
    Canon,
    Subordinate,
    Normal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionUrl {
    pub normalized_identity: String,
    pub location: PeerConnectionLocation,
    pub connection: PeerConnectionUrlSettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerConnectionLocation {
    Local(PeerConnectionLocalUrl),
    Sftp(PeerConnectionSftpUrl),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionLocalUrl {
    pub path_or_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionSftpUrl {
    pub host: String,
    pub username: String,
    pub password: Option<String>,
    pub port: u16,
    pub absolute_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerConnectionUrlSettings {
    pub timeout_conn_seconds: Option<u32>,
    pub timeout_idle_seconds: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerConnectionGlobalSettings {
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionRunMode {
    Normal,
    DryRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionLocalEnvironment {
    pub home_directory: PathBuf,
    pub known_hosts_path: PathBuf,
    pub ssh_agent_socket: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionStartupResult {
    pub reachable_peers: Vec<ReachablePeerConnection>,
    pub unreachable_peers: Vec<UnreachablePeerConnection>,
    pub status: PeerConnectionStartupStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReachablePeerConnection {
    pub peer_identity: String,
    pub role: PeerConnectionPeerRole,
    pub winning_url: PeerConnectionUrl,
    pub effective_sftp_connection: Option<PeerConnectionEffectiveSftpSettings>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerConnectionEffectiveSftpSettings {
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnreachablePeerConnection {
    pub peer_identity: String,
    pub role: PeerConnectionPeerRole,
    pub diagnostic: PeerConnectionDiagnostic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionDiagnostic {
    pub kind: PeerConnectionDiagnosticKind,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionDiagnosticKind {
    UnreachablePeer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerConnectionStartupStatus {
    Ready,
    Fatal(Vec<PeerConnectionFatalStartupReason>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionFatalStartupReason {
    FewerThanTwoReachablePeers,
    CanonPeerUnreachable,
}

pub trait PeerConnections: Send + Sync {
    /// Establishes startup reachability for an already-validated ordered peer
    /// list and returns the peers that may be used for the rest of the run.
    ///
    /// Connection work for every peer begins without waiting for any other
    /// peer to finish. Within one peer, URLs are tried sequentially in the
    /// supplied order: the primary URL first, followed by fallback URLs in
    /// command-line order. The first URL whose establishment succeeds becomes
    /// that peer's winning URL for this run, and no later fallback URL for that
    /// peer may be tried after the winner is selected. Reachable and
    /// unreachable result records preserve the caller's peer identity and role;
    /// records in each result list remain in the caller's peer order among
    /// their category.
    ///
    /// A reachable peer has exactly one winning URL. Later peer work must use
    /// the returned winning URL and must not re-select among fallback URLs. For
    /// a winning SFTP URL, the returned handle carries the effective connection
    /// timeout and idle keep-alive values: the URL value when present, or the
    /// matching global value when the URL omits it. For a winning `file://`
    /// URL, `effective_sftp_connection` is `None`, and timeout and SFTP idle
    /// settings do not affect establishment.
    ///
    /// For `file://` URLs, establishment is local path preparation. In normal
    /// mode, the peer root directory and missing parents are created before the
    /// URL is accepted. In dry-run mode, no local directory is created; a
    /// missing root fails only that URL. If normal-mode local root creation
    /// fails, only that URL fails and the next fallback URL may be tried.
    ///
    /// For `sftp://` URLs, establishment opens TCP, SSH, and SFTP, verifies
    /// that the server host key matches `known_hosts_path` for the contacted
    /// server and port, and authenticates before checking the remote peer root.
    /// An unknown, absent, or rejected host key fails only that URL. The SSH
    /// handshake is bounded by the URL `timeout_conn_seconds` when present, or
    /// by the global connection timeout otherwise. Authentication tries
    /// credential sources in this exact order: inline URL password, SSH agent,
    /// `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`. Absent or
    /// rejected credentials are skipped in favor of the next source, and
    /// authentication fails only after every listed source has been tried or
    /// skipped. In normal mode, a missing remote root and any missing parents
    /// are created through SFTP before the URL is accepted. In dry-run mode, no
    /// remote directory is created; a missing root fails only that URL.
    ///
    /// A peer is unreachable only when all of its URLs fail. Each unreachable
    /// peer produces exactly one error-level diagnostic in the startup result;
    /// diagnostics are structured data for the caller to print and this method
    /// does not own final stdout formatting. URL-level failures such as host
    /// key rejection, authentication exhaustion, handshake timeout, and root
    /// creation failure do not by themselves make startup fatal.
    ///
    /// After all peer attempts finish, startup status is `Ready` only when the
    /// canon peer is reachable and at least two peers are reachable. Startup
    /// status is `Fatal` with one or both fatal reasons when fewer than two
    /// peers are reachable or the canon peer is unreachable. Normal-mode calls
    /// are not side-effect-free because they may create only peer root
    /// directories and missing parents while accepting startup URLs; dry-run
    /// calls must not create peer-side directories.
    fn establish_peer_connections(
        &self,
        request: PeerConnectionStartupRequest,
    ) -> PeerConnectionStartupResult;
}
