use peertransportsurface::{ConnectedPeerRoot, PeerTransportError};
use snapshotdatabase::SnapshotDatabasePeerDatabase;

#[derive(Clone)]
pub struct SyncTraversalRequest {
    pub peers: Vec<SyncTraversalPeer>,
    pub retries_list: u64,
    pub excludes: Vec<String>,
}

#[derive(Clone)]
pub struct SyncTraversalPeer {
    pub peer_index: usize,
    pub peer_url: String,
    pub role: SyncTraversalPeerRole,
    pub had_snapshot_history: bool,
    pub root: ConnectedPeerRoot,
    pub snapshot_database: SnapshotDatabasePeerDatabase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncTraversalPeerRole {
    Normal,
    Canon,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncTraversalResult {
    pub diagnostics: Vec<SyncTraversalDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncTraversalDiagnostic {
    pub level: SyncTraversalDiagnosticLevel,
    pub peer_index: usize,
    pub path: Option<String>,
    pub kind: SyncTraversalDiagnosticKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncTraversalDiagnosticLevel {
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncTraversalDiagnosticKind {
    DirectoryListingFailed(PeerTransportError),
}

pub trait SyncTraversal: Send + Sync {
    /// Starts one recursive combined-tree traversal at the sync root for the
    /// reachable peers supplied in the request. The request must not contain
    /// peers that startup marked unreachable; omitted peers do not list, vote,
    /// receive file operations, or have snapshot rows changed by this child in
    /// this run. A peer that was unreachable in an earlier run participates
    /// normally when it is supplied to a later traversal. `path = None` in a
    /// returned diagnostic means the sync root, and `Some` is a validated
    /// relative slash path.
    ///
    /// At each visited directory level, listing work is started for every peer
    /// active for that subtree before any listing result from that level is
    /// awaited. Each listing is tried at most `retries_list` total times,
    /// including the first try. When a listing still fails, the result contains
    /// one error-level directory-listing diagnostic for that peer and path. A
    /// non-canon failed peer is removed from decisions and recursion for that
    /// directory and its subtree, and this child must not create, delete,
    /// displace, copy to, copy from, or update snapshot rows for that peer
    /// under the failed subtree in this run. If the canon peer fails to list a
    /// directory after all tries, the traversal skips decisions for that
    /// directory and its subtree on all peers and makes no peer-file or
    /// snapshot changes under that subtree. If all contributing peers fail for
    /// a directory, no entries in that directory or its subtree are processed.
    ///
    /// The traversal entry set for a directory is the union of child names from
    /// live listings of all active contributing peers and all active
    /// subordinate peers for that directory. Snapshot rows never add names to
    /// the traversal entry set. Built-in excludes and the request's accepted
    /// command-line excludes are removed before any reconciliation decision is
    /// made. Built-in excludes are `.kitchensync/` directories, `.git/`
    /// directories, symbolic links, and special files. Command-line excludes
    /// add skipped paths but never make a built-in excluded path syncable. A
    /// file exclude skips only that file; a directory exclude skips the
    /// directory and all descendants. Excluded paths are left unchanged on
    /// every peer, and this child must not consult or update snapshot rows for
    /// excluded paths during the run.
    ///
    /// Surviving names in one directory are processed in deterministic
    /// case-insensitive lexicographic order, with the original case-sensitive
    /// name as the tie-breaker. Every entry in a directory is processed before
    /// recursion into any child directory from that directory. The traversal
    /// never recurses into a directory that is displaced, and only peers that
    /// keep or receive a directory participate in recursion into that
    /// directory.
    ///
    /// For each visited path, the decision uses live entry state from current
    /// directory listings and the needed per-peer snapshot rows. Only
    /// contributing peers vote. A canon peer that is active for the subtree
    /// chooses the outcome directly: canon file means file, canon directory
    /// means directory, and canon absence means absence. A non-canon peer with
    /// no snapshot history at startup and a peer marked subordinate do not
    /// contribute in this run; subordinate peers still receive the outcome
    /// chosen from the active contributing peers.
    ///
    /// File decisions without a canon peer classify contributing peer state
    /// from live entries and snapshot rows. Matching unchanged file votes keep
    /// the unchanged file. A modified or new file more than five seconds newer
    /// than every other live file version wins. Deletion evidence uses the
    /// newest deletion estimate and wins only when it is more than five seconds
    /// newer than every contributing live file version; live file evidence wins
    /// exact ties and any value not more than five seconds older than the
    /// deletion estimate. Among live versions within five seconds of the
    /// newest modification time, larger byte size wins. If tied live versions
    /// have the same modification time within tolerance and the same byte size,
    /// each tied peer keeps its current bytes, and a peer lacking that file
    /// receives bytes from one tied source with the tied modification time and
    /// byte size. If no contributing peer votes for a file, absence is the
    /// outcome.
    ///
    /// Directory decisions without a canon peer ignore directory modification
    /// times. Live directories vote for existence regardless of their snapshot
    /// row. A contributing peer with no live directory and no snapshot row does
    /// not vote. If every contributing peer that votes has the directory live,
    /// the directory is the outcome. If no contributing peer has a live
    /// directory and every contributing peer with a row is absent, absence is
    /// the outcome. If no contributing peer has either a live directory or a
    /// snapshot row, absence is the outcome. When live directory evidence
    /// conflicts with deletion evidence, directory survival depends only on
    /// live files inside the directory subtree: no live files means absence,
    /// deletion more than five seconds newer than every live file means
    /// absence, and at least one live file not more than five seconds older
    /// than the deletion estimate means the directory survives and its children
    /// are reconciled by their own rules. If live subtree evidence cannot be
    /// fully listed after the configured listing tries, that directory subtree
    /// is left unreconciled for all peers in this run.
    ///
    /// Without a canon peer, if at least one contributing peer has a file and
    /// at least one contributing peer has a directory at the same path, the
    /// file type is the group outcome. The winning file content is then chosen
    /// by the normal file decision rules using only contributing file entries.
    /// A subordinate peer's file does not make the file type win over a
    /// contributing peer's directory, but any subordinate peer with the wrong
    /// type receives the type chosen from contributing peers.
    ///
    /// After choosing an outcome, this operation applies it to all active peers
    /// for that subtree, including subordinate peers, through the snapshot,
    /// staging, format-rules, and peer-transport boundaries. It requests
    /// snapshot row changes only after the corresponding listed state,
    /// intended file copy, completed inline directory creation, confirmed
    /// absence, or successful displacement is allowed by the traversal rules.
    /// It does not call local or SFTP transport implementations directly and
    /// does not parse command-line arguments, decide startup reachability,
    /// download or upload snapshots, or print the final completion line.
    fn traverse(&self, request: SyncTraversalRequest) -> SyncTraversalResult;
}
