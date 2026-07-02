use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorRequest {
    pub peers: Vec<StartupCoordinatorPeer>,
    pub global_connection: StartupCoordinatorGlobalSettings,
    pub run_mode: StartupCoordinatorRunMode,
    pub local_environment: StartupCoordinatorLocalEnvironment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorPeer {
    pub identity: String,
    pub role: StartupCoordinatorPeerRole,
    pub primary_url: StartupCoordinatorUrl,
    pub fallback_urls: Vec<StartupCoordinatorUrl>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupCoordinatorPeerRole {
    Canon,
    Subordinate,
    Normal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorUrl {
    pub normalized_identity: String,
    pub location: StartupCoordinatorUrlLocation,
    pub connection: StartupCoordinatorUrlSettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupCoordinatorUrlLocation {
    File(StartupCoordinatorFileUrl),
    Sftp(StartupCoordinatorSftpUrl),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorFileUrl {
    pub local_peer_root_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorSftpUrl {
    pub host: String,
    pub username: String,
    pub password: Option<String>,
    pub port: u16,
    pub absolute_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorUrlSettings {
    pub timeout_conn_seconds: Option<u32>,
    pub timeout_idle_seconds: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorGlobalSettings {
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupCoordinatorRunMode {
    Normal,
    DryRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorLocalEnvironment {
    pub home_directory: PathBuf,
    pub known_hosts_path: PathBuf,
    pub ssh_agent_socket: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorResult {
    pub reachable_peers: Vec<StartupCoordinatorReachablePeer>,
    pub unreachable_peers: Vec<StartupCoordinatorUnreachablePeer>,
    pub status: StartupCoordinatorStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorReachablePeer {
    pub peer_identity: String,
    pub role: StartupCoordinatorPeerRole,
    pub winning_url: StartupCoordinatorUrl,
    pub connection: StartupCoordinatorConnection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupCoordinatorConnection {
    File(StartupCoordinatorFileConnection),
    Sftp(StartupCoordinatorSftpConnection),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorFileConnection {
    pub local_peer_root_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorSftpConnection {
    pub host: String,
    pub username: String,
    pub port: u16,
    pub absolute_path: String,
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorUnreachablePeer {
    pub peer_identity: String,
    pub role: StartupCoordinatorPeerRole,
    pub diagnostic: StartupCoordinatorErrorDiagnostic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCoordinatorErrorDiagnostic {
    pub kind: StartupCoordinatorErrorDiagnosticKind,
    pub peer_identity: String,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupCoordinatorErrorDiagnosticKind {
    UnreachablePeer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupCoordinatorStatus {
    Ready,
    Fatal(Vec<StartupCoordinatorFatalReason>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupCoordinatorFatalReason {
    FewerThanTwoReachablePeers,
    CanonPeerUnreachable,
}

pub trait StartupCoordinator: Send + Sync {
    /// Coordinates startup URL selection for already-validated peers and
    /// returns the peers that are reachable for the rest of this run.
    ///
    /// Startup work for every peer begins without waiting for any other peer
    /// to finish. Within one peer, URL attempts are sequential: the primary URL
    /// is tried first, followed by fallback URLs in the caller-provided order.
    /// A URL failure affects only that URL attempt; it does not stop startup
    /// work for other peers and does not stop later fallback attempts for the
    /// same peer when another URL remains.
    ///
    /// Each URL is dispatched by kind. `file://` URLs are established by the
    /// file URL connection child, and `sftp://` URLs are established by the
    /// SFTP URL connection child. This operation treats each child success or
    /// failure as authoritative for that one URL attempt. It does not parse
    /// command-line text, validate peer arguments, normalize URL identity,
    /// choose peer roles, choose the canon peer, format output, create local
    /// directories itself, open SFTP sessions itself, check known hosts itself,
    /// choose SFTP credentials itself, or apply SSH timeout and keep-alive
    /// behavior itself.
    ///
    /// The first successful URL for a peer becomes that peer's only winning
    /// URL for this run. After a winner is recorded, no later fallback URL for
    /// that peer may be tried during this run. Reachable peer records preserve
    /// the caller's peer identity and role, return the winning URL, and return
    /// the connection handle or effective connection settings received for
    /// that winning URL. Later peer work must use the returned winning URL and
    /// connection data instead of re-running fallback selection.
    ///
    /// A peer is unreachable only when its primary URL and every fallback URL
    /// fail during startup. Each unreachable peer produces exactly one
    /// error-level diagnostic in the result. The diagnostic identifies the
    /// unreachable peer and is structured data for the caller; this operation
    /// does not print it.
    ///
    /// After all peer attempts finish, status is `Ready` only when at least
    /// two peers are reachable and the canon peer is reachable. Status is
    /// `Fatal` with one or both fatal reasons when fewer than two peers are
    /// reachable or when the canon peer is unreachable. This operation reports
    /// fatal startup status but never exits the process.
    ///
    /// Fallback selection is a startup-only decision. This operation does not
    /// retry fallback URLs after startup and does not reselect a different URL
    /// for a reachable peer after a winner has been chosen.
    fn coordinate_startup(
        &self,
        request: StartupCoordinatorRequest,
    ) -> StartupCoordinatorResult;
}
