use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryOutcomeRequest {
    pub relative_path: String,
    pub active_peers: Vec<DirectoryPeerInput>,
    pub canon_peer_identity: Option<String>,
    pub survival_evidence: DirectorySurvivalEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryPeerInput {
    pub peer_identity: String,
    pub role: DirectoryPeerRole,
    pub is_active_target: bool,
    pub has_live_directory: bool,
    pub snapshot: Option<DirectorySnapshotFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryPeerRole {
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectorySnapshotFact {
    pub deleted_time: Option<SystemTime>,
    pub last_seen: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectorySurvivalEvidence {
    NotNeeded,
    NoLiveFiles,
    NewestLiveFile {
        modification_time: SystemTime,
    },
    CollectionFailed {
        failed_peer_identities: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectoryOutcomeResult {
    Decision(DirectoryOutcomeDecision),
    SubtreeBlocked(DirectorySubtreeBlock),
    InvalidInput(DirectoryOutcomeInvalidInput),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryOutcomeDecision {
    pub relative_path: String,
    pub group_outcome: DirectoryGroupOutcome,
    pub peer_outcomes: Vec<DirectoryPeerOutcome>,
    pub creation_intents: Vec<DirectoryCreationIntent>,
    pub displacement_intents: Vec<DirectoryDisplacementIntent>,
    pub recursion: Option<DirectoryRecursion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryGroupOutcome {
    Exists,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryPeerOutcome {
    pub peer_identity: String,
    pub outcome: DirectoryPeerDirectoryOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryPeerDirectoryOutcome {
    KeepsDirectory,
    CreateDirectory,
    DirectoryAbsent,
    DisplaceDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryCreationIntent {
    pub peer_identity: String,
    pub relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryDisplacementIntent {
    pub peer_identity: String,
    pub relative_path: String,
    pub ordering: DirectoryDisplacementOrdering,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryDisplacementOrdering {
    WholeDirectoryPreOrder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryRecursion {
    pub relative_path: String,
    pub peer_identities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectorySubtreeBlock {
    pub relative_path: String,
    pub blocked_peer_identities: Vec<String>,
    pub reason: DirectorySubtreeBlockReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectorySubtreeBlockReason {
    SurvivalEvidenceCollectionFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryOutcomeInvalidInput {
    pub relative_path: String,
    pub reason: DirectoryOutcomeInvalidReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectoryOutcomeInvalidReason {
    EmptyPeerSet,
    DuplicatePeerIdentity(String),
    CanonPeerNotActive(String),
    MissingContributingPeerDeletionEstimate(String),
    SurvivalEvidenceSuppliedForNonConflict,
    SurvivalEvidenceMissingForLiveDirectoryConflict,
}

pub trait DirectoryOutcomes: Send + Sync {
    /// Selects the directory outcome for one non-excluded directory path and
    /// returns only structured planner facts and intents.
    ///
    /// The operation is a pure decision for the supplied facts: it must not
    /// list directories, read directory modification times, inspect file
    /// contents, create directories, move entries, update snapshots, format
    /// output, or fetch additional state. Repeating the same request returns
    /// the same decision.
    ///
    /// A canon peer, when supplied, wins unconditionally. A live canon
    /// directory makes the directory exist on every active target; missing
    /// active targets receive creation intents and recursion is eligible for
    /// peers that keep or create the directory. A missing canon path makes the
    /// path absent on every active peer; active peers that have the directory
    /// receive whole-directory pre-order displacement intents, no peer
    /// receives a creation intent, and recursion is not eligible.
    ///
    /// Without a canon peer, only contributing peers vote. A contributing
    /// peer with a live directory votes for existence even when its snapshot
    /// fact differs. A contributing peer with no live directory and no
    /// snapshot fact does not vote. Subordinate peers never vote, but active
    /// subordinate targets can receive creation or displacement intents after
    /// the contributing result is selected.
    ///
    /// If every voting contributing peer has the live directory, the
    /// directory exists on every active target, missing active targets receive
    /// creation intents, and recursion is eligible for peers that keep or
    /// create it. If no contributing peer has the directory live, at least one
    /// contributing peer has a snapshot fact, and every contributing peer with
    /// a snapshot fact is absent, active peers with the directory receive
    /// whole-directory pre-order displacement intents and recursion is not
    /// eligible. If no contributing peer has the directory live or in a
    /// snapshot fact, the group outcome is absence without contributing
    /// deletion history; subordinate peers that have the directory receive
    /// whole-directory pre-order displacement intents, no directory is
    /// created, and recursion is not eligible.
    ///
    /// When at least one contributing peer has the directory live and at least
    /// one voting contributing peer is absent, the operation treats the path
    /// as a live-directory deletion conflict. Each absent voting contributing
    /// peer contributes a deletion estimate from `deleted_time` when present,
    /// otherwise from `last_seen`; the newest estimate is used. Survival
    /// evidence is only the newest live file modification time under the live
    /// directory among peers that have it live. Directory modification times,
    /// child directory modification times, and empty live directory subtrees
    /// must not provide survival evidence.
    ///
    /// If survival-evidence collection failed after all allowed listing tries,
    /// the result is `DirectoryOutcomeResult::SubtreeBlocked`. That block
    /// means no active peer may receive file mutation intents, directory
    /// mutation intents, copy intents, displacement intents, creation intents,
    /// recursion work, or snapshot update intents anywhere under this
    /// directory subtree during the current run.
    ///
    /// In a live-directory deletion conflict, deletion wins when there is no
    /// survival evidence or when the newest deletion estimate exceeds the
    /// survival evidence by more than five seconds. Deletion winners displace
    /// every active peer that has the directory, do not recreate the directory
    /// on peers that lack it, and are not eligible for recursion. Otherwise
    /// the directory survives on every active target, missing active targets
    /// receive creation intents, and recursion remains eligible. Survival of
    /// the directory must not suppress child file decisions; newer child files
    /// remain eligible to propagate and older child files remain eligible for
    /// removal by the file rules during recursion.
    ///
    /// Every returned displacement intent is whole-directory and pre-order.
    /// A displaced directory is moved as one directory before any of its
    /// children can be independently visited, and the displaced peer must not
    /// be included in recursion for that directory.
    ///
    /// Invalid input returns `DirectoryOutcomeResult::InvalidInput` instead of
    /// inventing votes, sources, deletion estimates, or mutation intents. This
    /// includes duplicate peer identities, a canon identity that is not active
    /// for the path, a live-directory deletion conflict whose absent voting
    /// peer has neither `deleted_time` nor `last_seen`, missing survival
    /// evidence for a live-directory conflict, or survival evidence supplied
    /// for a non-conflict decision.
    fn decide_directory(&self, request: DirectoryOutcomeRequest) -> DirectoryOutcomeResult;
}
