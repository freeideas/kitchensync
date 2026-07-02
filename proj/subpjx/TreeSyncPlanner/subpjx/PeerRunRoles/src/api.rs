#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRunRolesRequest {
    pub peers: Vec<StartupPeerFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupPeerFact {
    pub peer_identity: String,
    pub reachability: StartupPeerReachability,
    pub role_marker: StartupPeerRoleMarker,
    pub had_snapshot_database_at_startup: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPeerReachability {
    Reachable,
    Unreachable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPeerRoleMarker {
    Normal,
    Canon,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerRunRolesResult {
    Success(PeerRunRolesFacts),
    FatalStartup(PeerRunRolesFatalStartup),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRunRolesFacts {
    pub active_peers: Vec<PeerRunRoleFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRunRoleFact {
    pub peer_identity: String,
    pub is_canon: bool,
    pub role: PeerRunRole,
    pub is_active_target: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRunRole {
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerRunRolesFatalStartup {
    UnreachableCanon {
        exit_status: i32,
        canon_peer_identity: String,
    },
    FirstSyncRequiresCanon {
        exit_status: i32,
        stdout_line: String,
    },
    NoContributingPeer {
        exit_status: i32,
        stdout_line: String,
    },
}

pub trait PeerRunRoles: Send + Sync {
    /// Classifies all configured peers for one KitchenSync run.
    ///
    /// The operation is a pure startup decision over only the supplied
    /// reachability, role-marker, and snapshot-existence facts. It must
    /// not parse command-line text, normalize peer URLs, connect to peers,
    /// inspect files, read snapshot rows, list directories, update snapshots,
    /// decide sync outcomes, execute mutations, or format stdout beyond the
    /// fatal stdout lines returned in `PeerRunRolesFatalStartup`. Repeating the
    /// same request returns the same result, and role classifications from
    /// previous runs must not affect the current run.
    ///
    /// A peer marked canon but unreachable returns
    /// `PeerRunRolesFatalStartup::UnreachableCanon` with exit status `1`
    /// before any successful role result is returned. That fatal result has no
    /// stdout line obligation from this child. If no reachable peer has startup
    /// snapshot data and no peer is marked canon, the result is
    /// `PeerRunRolesFatalStartup::FirstSyncRequiresCanon` with exit status `1`
    /// and stdout line
    /// `First sync? Mark the authoritative peer with a leading +`. After that
    /// first-sync case is ruled out, if automatic subordination leaves no
    /// reachable contributing peer, the result is
    /// `PeerRunRolesFatalStartup::NoContributingPeer` with exit status `1` and
    /// stdout line
    /// `No contributing peer reachable - cannot make sync decisions`.
    ///
    /// A successful result contains exactly the reachable peers for the run as
    /// active peers. Unreachable non-fatal peers are omitted and must not be
    /// eligible for listing requests, sync decision inputs, sync decision
    /// targets, or snapshot updates in that run. Each returned active peer is
    /// an active target.
    ///
    /// A reachable canon peer is returned with `is_canon == true`,
    /// `PeerRunRole::Contributing`, and unconditional authority for sibling
    /// conflict planners even when its snapshot database did not exist at
    /// startup. A reachable non-canon peer with no startup snapshot database is
    /// subordinate. A reachable peer marked subordinate is subordinate even
    /// when it has snapshot history. A reachable normal non-canon peer with
    /// startup snapshot history is contributing. Subordinate peers never vote,
    /// but remain active targets for the selected contributing or canon
    /// outcome.
    fn classify_startup_roles(&self, request: PeerRunRolesRequest) -> PeerRunRolesResult;
}
