#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraverseDirectoryRequest {
    pub relative_directory_path: String,
    pub active_peers: Vec<TreeTraversalPeer>,
    pub accepted_excludes: Vec<AcceptedExclude>,
    pub list_total_tries: u32,
    pub directory_listing_facts: Vec<DirectoryListingFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeTraversalPeer {
    pub peer_id: String,
    pub role: TreeTraversalPeerRole,
    pub is_canon: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeTraversalPeerRole {
    Contributing,
    Subordinate,
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
pub struct TraverseDirectoryResult {
    pub relative_directory_path: String,
    pub diagnostics: Vec<TreeTraversalDiagnostic>,
    pub listing_failures: Vec<DirectoryListingFailureFact>,
    pub run_local_exclusions: Vec<RunLocalPeerExclusion>,
    pub subtree_skips: Vec<SubtreeSkipIntent>,
    pub entries: Vec<EntryProcessingFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeTraversalDiagnostic {
    pub level: TreeTraversalDiagnosticLevel,
    pub kind: TreeTraversalDiagnosticKind,
    pub peer_id: Option<String>,
    pub relative_path: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeTraversalDiagnosticLevel {
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeTraversalDiagnosticKind {
    DirectoryListingFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryListingFailureFact {
    pub peer_id: String,
    pub relative_directory_path: String,
    pub tries_used: u32,
    pub diagnostic: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLocalPeerExclusion {
    pub peer_id: String,
    pub relative_directory_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtreeSkipIntent {
    pub relative_directory_path: String,
    pub peer_ids: Vec<String>,
    pub reason: SubtreeSkipReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtreeSkipReason {
    CanonListingFailed,
    AllContributingListingsFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryProcessingFact {
    pub relative_path: String,
    pub entry_name: String,
    pub peer_facts: Vec<EntryPeerFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPeerFact {
    pub peer_id: String,
    pub role: TreeTraversalPeerRole,
    pub is_canon: bool,
    pub live_entry: Option<PeerLiveEntry>,
    pub eligibility: EntryPeerEligibility,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerLiveEntry {
    pub name: String,
    pub kind: LiveEntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPeerEligibility {
    pub snapshot_lookup: bool,
    pub snapshot_update: bool,
    pub file_mutation: bool,
    pub directory_mutation: bool,
    pub copy: bool,
    pub deletion: bool,
    pub displacement: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildRecursionRequest {
    pub parent_relative_directory_path: String,
    pub processed_entries: Vec<ProcessedEntryRecursionFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedEntryRecursionFact {
    pub relative_path: String,
    pub peer_decisions: Vec<ChildDirectoryPeerDecision>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildDirectoryPeerDecision {
    pub peer_id: String,
    pub disposition: ChildDirectoryDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildDirectoryDisposition {
    KeepsDirectory,
    CreatesDirectory,
    DirectoryAbsent,
    DisplacesDirectory,
    NotAChildDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildRecursionIntent {
    pub relative_directory_path: String,
    pub peer_ids: Vec<String>,
}

pub trait TreeTraversal: Send + Sync {
    /// Traverses one sync-root or child directory using the supplied active
    /// peer facts, accepted excludes, retry count, and directory listing
    /// facts.
    ///
    /// Listing facts are interpreted as the results of starting a listing
    /// request for every peer still active in the subtree before awaiting any
    /// listing result for this directory. Retry counts are per peer and per
    /// directory path. A failed listing is final only when it represents all
    /// allowed tries for the same peer and path; final listing failures are
    /// returned as `listing_failures` and error diagnostics, never as copy work.
    ///
    /// A non-canon peer whose listing fails after all allowed tries is removed
    /// from run-local eligibility for this directory and every descendant path;
    /// the result records that removal in `run_local_exclusions`. The removal
    /// is only for the current run. When the canon peer fails, or when every
    /// contributing peer fails, the result returns a subtree skip for this
    /// directory and all descendants for every active peer. A skipped subtree
    /// returns no entry-processing facts and gives no peer mutation, copy,
    /// snapshot lookup, or snapshot update eligibility under the skipped path.
    ///
    /// Entry-processing facts are formed only from successful live listing
    /// results for peers that remain active at this directory. Snapshot-only
    /// paths do not add names. Active contributing and subordinate peers both
    /// contribute live entry names. Entries are returned in deterministic
    /// case-insensitive lexicographic order, using the original case-sensitive
    /// name as the tie-breaker.
    ///
    /// Accepted file excludes hide only the matching file path. Accepted
    /// directory excludes hide the directory and all descendants. Built-in
    /// excludes always hide `.kitchensync/` directories, `.git/` directories,
    /// symbolic link files, symbolic link directories, and special files, and
    /// command-line excludes cannot override them.
    ///
    /// Excluded paths produce no decision item, peer eligibility, snapshot
    /// lookup eligibility, snapshot update eligibility, copy eligibility,
    /// deletion eligibility, displacement eligibility, scan request, or
    /// recursion input. Excluded directories are not scanned and are not
    /// recursed into, even when another peer's live listing would otherwise
    /// create, delete, copy, or displace that path.
    ///
    /// Each returned entry-processing fact contains the path, the sorted entry
    /// name, each remaining peer's exact live name and kind when that peer
    /// contributed the entry, and the peer eligibility facts needed by sibling
    /// outcome planners. The method does not consult snapshot rows and only
    /// marks which non-excluded paths remain eligible for snapshot lookup and
    /// update by the parent facade or another planner.
    fn traverse_directory(&self, request: TraverseDirectoryRequest) -> TraverseDirectoryResult;

    /// Builds child-recursion intents from the parent facade's completed
    /// directory or type-conflict decisions for every entry in one directory.
    ///
    /// This operation is called only after the parent facade has processed all
    /// entry-processing facts for the directory. It must not cause recursion
    /// to begin before every entry in the current directory has a supplied
    /// decision fact.
    ///
    /// A child-recursion intent is returned only for a child directory path
    /// with at least one peer whose completed decision keeps or creates that
    /// child directory. Peers whose decision displaces the directory, leaves it
    /// absent, or resolves the entry as not being a child directory are omitted
    /// from that recursion intent. If no peer remains eligible for a child
    /// directory, no recursion intent is returned for that path.
    ///
    /// Returned intents preserve the parent facade's decisions exactly: this
    /// method does not choose file winners, directory winners, type-conflict
    /// winners, copy sources, BAK moves, snapshot rows, or additional scans.
    fn plan_child_recursions(
        &self,
        request: ChildRecursionRequest,
    ) -> Vec<ChildRecursionIntent>;
}
