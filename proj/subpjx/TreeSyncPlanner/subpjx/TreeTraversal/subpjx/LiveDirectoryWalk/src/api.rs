pub type LiveDirectoryListingStarter = std::sync::Arc<
    dyn Fn(LiveDirectoryListingAttemptRequest) -> LiveDirectoryListingCompletion + Send + Sync,
>;

pub type LiveDirectoryListingCompletion =
    Box<dyn FnOnce() -> Result<Vec<LiveDirectoryListedEntry>, LiveDirectoryListingError> + Send>;

#[derive(Clone)]
pub struct LiveDirectoryWalkDirectoryRequest {
    pub relative_directory_path: String,
    pub active_peers: Vec<LiveDirectoryWalkPeer>,
    pub list_total_tries: u32,
}

#[derive(Clone)]
pub struct LiveDirectoryWalkPeer {
    pub peer_id: String,
    pub role: LiveDirectoryWalkPeerRole,
    pub is_canon: bool,
    pub listing_starter: LiveDirectoryListingStarter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDirectoryWalkPeerRole {
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryListingAttemptRequest {
    pub peer_id: String,
    pub relative_directory_path: String,
    pub attempt_number: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryListedEntry {
    pub name: String,
    pub kind: LiveDirectoryListedEntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveDirectoryListedEntryKind {
    File {
        byte_size: u64,
        modified_time: LiveDirectoryTimestamp,
    },
    Directory,
    SymbolicLinkFile,
    SymbolicLinkDirectory,
    Special,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LiveDirectoryTimestamp {
    pub unix_seconds: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryListingError {
    pub category: LiveDirectoryListingErrorCategory,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDirectoryListingErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryWalkDirectoryResult {
    pub diagnostics: Vec<LiveDirectoryWalkDiagnostic>,
    pub failed_subtrees: Vec<LiveDirectoryFailedSubtreeFact>,
    pub subtree_skips: Vec<LiveDirectorySubtreeSkipFact>,
    pub entry_facts: Vec<LiveDirectoryEntryFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryWalkDiagnostic {
    pub level: LiveDirectoryWalkDiagnosticLevel,
    pub kind: LiveDirectoryWalkDiagnosticKind,
    pub peer_id: Option<String>,
    pub relative_directory_path: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDirectoryWalkDiagnosticLevel {
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDirectoryWalkDiagnosticKind {
    DirectoryListingFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryFailedSubtreeFact {
    pub peer_id: String,
    pub relative_directory_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectorySubtreeSkipFact {
    pub relative_directory_path: String,
    pub peer_ids: Vec<String>,
    pub reason: LiveDirectorySubtreeSkipReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDirectorySubtreeSkipReason {
    CanonListingFailed,
    AllContributingPeersFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryEntryFact {
    pub relative_directory_path: String,
    pub entry_name: String,
    pub peer_entries: Vec<LiveDirectoryPeerEntryFact>,
    pub peer_eligibility: Vec<LiveDirectoryPeerEligibility>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryPeerEntryFact {
    pub peer_id: String,
    pub kind: LiveDirectoryListedEntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryPeerEligibility {
    pub peer_id: String,
    pub role: LiveDirectoryWalkPeerRole,
    pub is_canon: bool,
    pub eligible: bool,
    pub reason: LiveDirectoryPeerEligibilityReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDirectoryPeerEligibilityReason {
    ListedDirectory,
    ListingFailedForSubtree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryRecursionRequest {
    pub relative_directory_path: String,
    pub processed_entries: Vec<LiveDirectoryProcessedEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryProcessedEntry {
    pub entry_fact: LiveDirectoryEntryFact,
    pub parent_decision: LiveDirectoryParentEntryDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveDirectoryParentEntryDecision {
    NotChildDirectory,
    ChildDirectory {
        peer_decisions: Vec<LiveDirectoryPeerChildDirectoryDecision>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryPeerChildDirectoryDecision {
    pub peer_id: String,
    pub outcome: LiveDirectoryPeerChildDirectoryOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDirectoryPeerChildDirectoryOutcome {
    KeepDirectory,
    CreateDirectory,
    DisplaceDirectory,
    NoDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDirectoryRecursionIntent {
    pub relative_directory_path: String,
    pub peer_ids: Vec<String>,
}

pub trait LiveDirectoryWalk: Send + Sync {
    /// Lists one directory path for the peers that are active in that subtree.
    ///
    /// For the first try at the directory, the listing starter for every
    /// reachable active peer is called before any returned completion is
    /// awaited. Failed listings are retried for the same peer and directory
    /// path until that peer succeeds or reaches `list_total_tries`; retry
    /// counts are tracked per peer and per directory path. Listing failures
    /// are returned only as diagnostic and failed-subtree facts and must never
    /// be represented as file-copy work.
    ///
    /// A non-canon peer that still cannot list the directory after all allowed
    /// tries is removed from the live entry set for this directory and from
    /// descendant walks for the current run only. When at least one
    /// contributing peer remains active, entry facts are still produced from
    /// the successful active peers, and each failed peer receives a
    /// run-local failed-subtree fact that blocks decisions, file mutation,
    /// directory mutation, and snapshot row updates at this directory and
    /// every descendant path for that peer.
    ///
    /// If the canon peer still cannot list the directory after all allowed
    /// tries, the result contains a subtree-skip fact for every peer at this
    /// directory and all descendants, and contains no entry facts for the
    /// skipped path. If every contributing peer still cannot list the
    /// directory after all allowed tries, the result contains a subtree-skip
    /// fact for every peer, blocks subordinate cleanup under that subtree
    /// including subordinate displacement, and contains no entry facts for the
    /// skipped path.
    ///
    /// Entry names are formed only from live listing results, never from
    /// snapshot-only paths. Active contributing peers and active subordinate
    /// peers both contribute live entry names. Returned entry facts within the
    /// directory are ordered by case-insensitive lexicographic order, using
    /// the original case-sensitive name as the tie-breaker, and that order is
    /// stable regardless of listing completion order.
    fn list_directory(
        &self,
        request: LiveDirectoryWalkDirectoryRequest,
    ) -> LiveDirectoryWalkDirectoryResult;

    /// Forms child-recursion intents after the parent has processed a whole
    /// directory's entry facts.
    ///
    /// The request represents the fully processed entry set for one directory;
    /// no child recursion intent may be emitted until every entry in that
    /// directory has a parent decision. Only entries whose parent decision is
    /// `ChildDirectory` can produce recursion. For each such child directory,
    /// the returned intent includes only peers whose parent decision keeps or
    /// creates that child directory. Peers whose decision displaces the child
    /// directory are omitted, and a child directory with no remaining peer does
    /// not produce an intent.
    ///
    /// Returned recursion intents follow the order of `processed_entries` and
    /// have no side effect: the operation does not list directories, inspect
    /// snapshots, choose sync outcomes, create directories, displace paths,
    /// enqueue copies, or update snapshot rows.
    fn form_child_recursion_intents(
        &self,
        request: LiveDirectoryRecursionRequest,
    ) -> Vec<LiveDirectoryRecursionIntent>;
}
