use peertransportsurface::ConnectedPeerRoot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionsStartupRequest {
    pub dry_run: bool,
    pub timeout_conn_seconds: u64,
    pub timeout_idle_seconds: u64,
    pub peer_arguments: Vec<String>,
}

#[derive(Clone)]
pub enum PeerConnectionsStartupResult {
    Ready(PeerConnectionsStartup),
    Failed(PeerConnectionsStartupFailure),
}

#[derive(Clone)]
pub struct PeerConnectionsStartup {
    pub peers: Vec<PeerConnectionsPeer>,
    pub diagnostics: Vec<PeerConnectionsDiagnostic>,
}

#[derive(Clone)]
pub struct PeerConnectionsPeer {
    pub peer_index: usize,
    pub role: PeerConnectionsPeerRole,
    pub had_snapshot_history: bool,
    pub winning_url: String,
    pub root: ConnectedPeerRoot,
    pub snapshot_database: PeerConnectionsSnapshotDatabase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionsPeerRole {
    Normal,
    Canon,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionsSnapshotDatabase {
    pub path: std::path::PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionsDiagnostic {
    pub level: PeerConnectionsDiagnosticLevel,
    pub peer_index: usize,
    pub kind: PeerConnectionsDiagnosticKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionsDiagnosticLevel {
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionsDiagnosticKind {
    PeerUnreachable,
    SnapshotStartupFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConnectionsStartupFailure {
    pub reason: PeerConnectionsStartupFailureReason,
    pub diagnostics: Vec<PeerConnectionsDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerConnectionsStartupFailureReason {
    FewerThanTwoReachablePeers,
    CanonPeerUnreachable,
    FirstSyncNeedsCanon,
    NoContributingPeerReachable,
}

pub trait PeerConnections: Send + Sync {
    /// Starts peer connection selection and snapshot preparation for one run.
    /// The peer arguments are already accepted by the command-line layer, but
    /// this operation owns grouping: an argument without brackets is one peer
    /// with one candidate URL, an argument with brackets is one peer whose
    /// candidates are the bracket contents in written order, and leading `+`
    /// or `-` applies to the whole peer. Candidate URL order is never
    /// reordered. Connection establishment begins for all peers in parallel.
    /// Within one peer, the primary URL is tried first, fallback URLs are
    /// tried in written order, the first candidate that connects and satisfies
    /// root setup rules becomes the immutable winning URL for that run, and no
    /// later candidate for that peer is tried after a winner is selected. If
    /// root creation fails in a normal run, that candidate is failed and the
    /// next fallback may be tried. Later listing or transfer failures never
    /// restart startup fallback selection and never switch a reachable peer to
    /// another URL.
    ///
    /// Unreachable non-canon peers are skipped and reported as error-level
    /// diagnostics. Startup fails if fewer than two peers are reachable or if
    /// the canon peer is unreachable. For each reachable peer, normal runs
    /// perform snapshot SWAP recovery before snapshot download, while dry runs
    /// skip peer-side SWAP recovery and download the live snapshot as-is when
    /// present. A missing `.kitchensync/snapshot.db` keeps the peer reachable
    /// and prepares a new empty local snapshot database. Any snapshot SWAP
    /// recovery or snapshot download failure other than not found excludes
    /// that peer, reports an error-level diagnostic, and repeats the reachable
    /// set checks.
    ///
    /// Snapshot history means `.kitchensync/snapshot.db` existed on disk at
    /// the start of that peer's snapshot download step, even if its `snapshot`
    /// table had no rows. A reachable non-canon peer without snapshot history
    /// is automatically subordinate for this run; a reachable canon peer
    /// without snapshot history is not. Startup fails before reconciliation if
    /// no reachable peer had snapshot history and no canon peer was
    /// designated; that failure requires the exact stdout line `First sync?
    /// Mark the authoritative peer with a leading +`. Startup also fails
    /// before reconciliation if every reachable peer is subordinate; that
    /// failure requires the exact stdout line `No contributing peer reachable
    /// - cannot make sync decisions`.
    ///
    /// A ready result contains only peers that remain reachable after all
    /// startup checks. It never contains fewer than two peers, an unreachable
    /// canon peer, all subordinate peers, or a no-history/no-canon state. Each
    /// returned peer keeps its stable accepted-argument index, final role,
    /// snapshot-history flag, winning normalized URL, connected transport
    /// handle, and prepared local snapshot database.
    fn start(
        &self,
        request: PeerConnectionsStartupRequest,
    ) -> PeerConnectionsStartupResult;
}
