#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupRoleRequest {
    pub reachable_peers: Vec<StartupPeer>,
    pub designated_canon_peer_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupPeer {
    pub peer_id: String,
    pub command_line_role: StartupPeerCommandRole,
    pub had_snapshot_at_startup: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPeerCommandRole {
    Normal,
    Canon,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupRoleDecision {
    pub peer_roles: Vec<PeerRunRoleDecision>,
    pub fatal_outcome: Option<StartupFatalOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRunRoleDecision {
    pub peer_id: String,
    pub role: PeerRunRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRunRole {
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupFatalOutcome {
    FewerThanTwoReachablePeers {
        exit_code: i32,
    },
    FirstSyncRequiresCanon {
        exit_code: i32,
        stdout_line: String,
    },
    UnreachableCanon {
        exit_code: i32,
        canon_peer_id: String,
    },
    NoContributingPeer {
        exit_code: i32,
        stdout_line: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSyncPlanRequest {
    pub peers: Vec<PlanningPeer>,
    pub accepted_excludes: Vec<AcceptedExclude>,
    pub directory_listing_facts: Vec<DirectoryListingFact>,
    pub snapshot_facts: Vec<SnapshotFact>,
    pub list_total_tries: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanningPeer {
    pub peer_id: String,
    pub role: PeerRunRole,
    pub is_canon: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedExclude {
    pub relative_path: String,
    pub kind: AcceptedExcludeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptedExcludeKind {
    File,
    DirectorySubtree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryListingFact {
    pub peer_id: String,
    pub relative_directory_path: String,
    pub tries_used: u32,
    pub outcome: DirectoryListingOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectoryListingOutcome {
    Entries(Vec<LiveDirectoryEntry>),
    Failed {
        diagnostic: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryEntry {
    pub name: String,
    pub kind: LiveEntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveEntryKind {
    File {
        byte_size: u64,
        modified_time: SyncTimestamp,
    },
    Directory,
    SymbolicLinkFile,
    SymbolicLinkDirectory,
    Special,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyncTimestamp {
    pub unix_seconds: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotFact {
    pub peer_id: String,
    pub relative_path: String,
    pub row: SnapshotRow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRow {
    pub kind: SnapshotEntryKind,
    pub byte_size: Option<u64>,
    pub modified_time: Option<SyncTimestamp>,
    pub deleted_time: Option<SyncTimestamp>,
    pub last_seen: Option<SyncTimestamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotEntryKind {
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSyncPlan {
    pub diagnostics: Vec<TreeSyncDiagnostic>,
    pub actions: Vec<TreeSyncAction>,
    pub snapshot_update_intents: Vec<SnapshotUpdateIntent>,
    pub directory_visit_intents: Vec<DirectoryVisitIntent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSyncDiagnostic {
    pub level: TreeSyncDiagnosticLevel,
    pub kind: TreeSyncDiagnosticKind,
    pub peer_id: Option<String>,
    pub relative_path: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSyncDiagnosticLevel {
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSyncDiagnosticKind {
    DirectoryListingFailed,
    SurvivalEvidenceListingFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeSyncAction {
    CopyFile(FileCopyIntent),
    CreateDirectory(DirectoryCreateIntent),
    DisplacePath(PathDisplacementIntent),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCopyIntent {
    pub source_peer_id: String,
    pub source_relative_path: String,
    pub destination_peer_id: String,
    pub destination_relative_path: String,
    pub winning_byte_size: u64,
    pub winning_modified_time: SyncTimestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryCreateIntent {
    pub peer_id: String,
    pub relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathDisplacementIntent {
    pub peer_id: String,
    pub relative_path: String,
    pub kind: DisplacementKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplacementKind {
    File,
    DirectoryWholeSubtree,
    AnyExistingPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotUpdateIntent {
    UpsertFile {
        peer_id: String,
        relative_path: String,
        byte_size: u64,
        modified_time: SyncTimestamp,
    },
    UpsertDirectory {
        peer_id: String,
        relative_path: String,
    },
    Tombstone {
        peer_id: String,
        relative_path: String,
        deleted_time: SyncTimestamp,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryVisitIntent {
    pub relative_path: String,
    pub peer_ids: Vec<String>,
}

pub trait TreeSyncPlanner: Send + Sync {
    /// Selects the run role for each reachable startup peer.
    ///
    /// A reachable peer whose command-line role is canon, or whose identity is
    /// the designated canon peer, is contributing even when it had no snapshot
    /// database at startup. A reachable non-canon peer with no startup snapshot
    /// database is subordinate for the run. A reachable peer marked subordinate
    /// on the command line is subordinate even when it has snapshot history. A
    /// reachable peer with snapshot history and no subordinate marker is
    /// contributing. Unreachable peers must not appear in the request and
    /// therefore receive no returned role, listing work, decision, or snapshot
    /// update intent for the run.
    ///
    /// If fewer than two peers are reachable, the fatal outcome has exit code
    /// `1`. If no reachable peer has snapshot data and no canon peer is
    /// designated, the fatal outcome has exit code `1` and stdout line `First
    /// sync? Mark the authoritative peer with a leading +`. If the designated
    /// canon peer is not reachable, the fatal outcome has exit code `1`. If
    /// automatic subordination leaves no reachable contributing peer, the fatal
    /// outcome has an error exit and stdout line `No contributing peer
    /// reachable - cannot make sync decisions`. A run with at least one
    /// reachable contributing peer with snapshot history does not require a
    /// canon peer.
    fn decide_startup_roles(&self, request: StartupRoleRequest) -> StartupRoleDecision;

    /// Plans sync work for the root and all visible descendant paths.
    ///
    /// For each traversed directory, listing facts are interpreted as if a
    /// request was started for every peer still active in that subtree before
    /// any listing result was consumed. A listing failure for the same peer and
    /// path is honored only after up to `list_total_tries` total tries. Listing
    /// failures are returned only as error diagnostics, never as file-copy
    /// work.
    ///
    /// When a non-canon peer cannot list a directory after all allowed tries,
    /// that peer is excluded from decisions for that directory and every
    /// descendant in this run. When the canon peer fails, when every
    /// contributing peer fails, or when survival-evidence listing fails, the
    /// affected subtree produces no peer mutation or snapshot update intents
    /// for any peer. These exclusions are run-local.
    ///
    /// Live listing names, not snapshot-only names, drive traversal. Accepted
    /// command-line excludes are applied before snapshot lookup and before any
    /// decision: excluded paths are not scanned, recursed into, copied,
    /// displaced, created, used for decisions, used for snapshot lookup, or
    /// used for snapshot update intents. Built-in excludes always hide
    /// `.kitchensync/`, `.git/`, symbolic links, and special files.
    ///
    /// Entries within one directory are processed in case-insensitive
    /// lexicographic order, using the original case-sensitive name as the
    /// tie-breaker. The returned actions are pre-order: a directory selected
    /// for displacement is represented as one whole-subtree displacement before
    /// any child can be visited, and recursion is represented only for peers
    /// that keep or create that child directory. Synced filenames preserve the
    /// exact case reported by the selected source filesystem.
    ///
    /// Subordinate peers are listed and targeted but never vote. With a canon
    /// peer, the canon state wins for files, directories, and type conflicts.
    /// Without a canon peer, contributing peers select the outcome using the
    /// file, directory, deletion, and file-versus-directory rules from the
    /// specification. The 5-second tolerance is applied consistently to file
    /// classification, live-file vote comparison, deletion-versus-file
    /// comparison, and absent-unconfirmed deletion votes. Files tied on
    /// modification time and byte size are treated as identical even when their
    /// bytes differ.
    fn plan_sync_root(&self, request: TreeSyncPlanRequest) -> TreeSyncPlan;
}
