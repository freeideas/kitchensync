#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyncTimestamp {
    pub unix_seconds: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupFileDecisionRequest {
    pub relative_path: String,
    pub peers: Vec<GroupFileDecisionPeer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupFileDecisionPeer {
    pub peer_id: String,
    pub role: GroupFileDecisionPeerRole,
    pub classification: PeerFileState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupFileDecisionPeerRole {
    Canon,
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerFileState {
    UnchangedLiveFile(ClassifiedLiveFile),
    ModifiedLiveFile(ClassifiedLiveFile),
    NewLiveFile(ClassifiedLiveFile),
    DeletedFile {
        deletion_estimate: SyncTimestamp,
    },
    AbsentUnconfirmed {
        last_seen: Option<SyncTimestamp>,
    },
    AbsentNoRowNoVote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedLiveFile {
    pub byte_size: u64,
    pub modified_time: SyncTimestamp,
    pub source_relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupFileDecisionOutput {
    pub relative_path: String,
    pub group_outcome: FileGroupOutcome,
    pub source_peers: Vec<FileOutcomeSource>,
    pub copy_intents: Vec<FileCopyIntent>,
    pub absence_intents: Vec<FileAbsenceIntent>,
    pub peer_decisions: Vec<PeerFileDecisionFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileGroupOutcome {
    ExistingFile {
        byte_size: u64,
        modified_time: SyncTimestamp,
    },
    Deletion {
        deletion_estimate: Option<SyncTimestamp>,
    },
    NoFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOutcomeSource {
    pub peer_id: String,
    pub source_relative_path: String,
    pub byte_size: u64,
    pub modified_time: SyncTimestamp,
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
pub enum FileAbsenceIntent {
    DeleteFile {
        peer_id: String,
        relative_path: String,
    },
    DisplaceFile {
        peer_id: String,
        relative_path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerFileDecisionFact {
    pub peer_id: String,
    pub role: GroupFileDecisionPeerRole,
    pub classification: PeerFileState,
    pub statuses: Vec<PeerFileDecisionStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerFileDecisionStatus {
    CanonSelectedOutcome,
    VotedForExistingFile,
    VotedForDeletion,
    DidNotVote,
    MatchedWinner,
    IdenticalSource,
    SelectedAsCopySource,
    NeedsCopy,
    NotSelectedForCopy,
    NeedsDeletion,
    NeedsDisplacement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupFileDecisionError {
    InvalidInput(String),
}

pub trait GroupFileDecision: Send + Sync {
    /// Selects the group outcome and planner facts for one already-visible file
    /// path from supplied peer classifications and role facts.
    ///
    /// The request must describe exactly one file path across active peers. A
    /// canon peer, when present, selects the final outcome without consulting
    /// non-canon peers: a canon live file becomes the file outcome for every
    /// other active peer, while a canon state without a live file selects
    /// deletion for every other active peer that has a live file. Non-canon
    /// peers must not change a canon file decision.
    ///
    /// Without a canon peer, only contributing peers vote. Subordinate peers
    /// never vote, but active subordinate peers can receive copy or
    /// displacement intents after the contributing outcome is selected. If all
    /// contributing peers that have a file are unchanged and matching, that
    /// unchanged file is the group outcome. If every contributing peer is
    /// absent with no snapshot row, the outcome is no file, no copy intent is
    /// returned, and active subordinate live files are selected for
    /// displacement.
    ///
    /// Live-file votes are compared by modification time with an inclusive
    /// 5-second tolerance around the maximum time. A time more than 5 seconds
    /// behind the maximum loses to the maximum. Among tied live-file votes, the
    /// larger byte size wins. Tied live files with equal byte size are treated
    /// as identical for planning even when their bytes differ. No copy intent
    /// is selected between identical live files, and a target that needs a file
    /// identical on multiple source peers may copy from any one of those source
    /// peers. A peer that already has the winning byte size and a live modified
    /// time within 5 seconds of the winner is not selected for copy.
    ///
    /// When deleted votes and existing-file votes both exist, the most recent
    /// deletion estimate is compared with the winning existing-file modified
    /// time. A deletion estimate more than 5 seconds newer than the existing
    /// file selects deletion. An existing file whose modified time is not more
    /// than 5 seconds older than the deletion estimate wins over deletion,
    /// including exact ties. An absent-unconfirmed contributing peer votes for
    /// deletion only when its `last_seen` is present and more than 5 seconds
    /// newer than the maximum live-file modified time; otherwise it does not
    /// vote and receives the file when an existing file wins.
    ///
    /// Deletion outcomes produce deletion or displacement intents only, never
    /// copy intents. Every returned outcome, source fact, peer decision fact,
    /// and intent must refer to the single input path and has no execution side
    /// effect. The method must not inspect files, fetch metadata, compare file
    /// bytes, read or write snapshots, copy, delete, displace, set timestamps,
    /// format process output, or decide process exit status.
    ///
    /// Returns `GroupFileDecisionError::InvalidInput` when the supplied facts
    /// cannot describe one coherent group decision for one file path, when a
    /// required canon or contributing role fact is contradictory, or when a
    /// live vote is missing the byte size or modification time needed for
    /// comparison. The method must not invent metadata, fetch additional state,
    /// or silently treat malformed facts as votes.
    fn decide_group_file(
        &self,
        request: GroupFileDecisionRequest,
    ) -> Result<GroupFileDecisionOutput, GroupFileDecisionError>;
}
