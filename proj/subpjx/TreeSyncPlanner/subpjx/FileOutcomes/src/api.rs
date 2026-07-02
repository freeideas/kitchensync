#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyncTimestamp {
    pub unix_seconds: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerFileClassificationRequest {
    pub peer_id: String,
    pub relative_path: String,
    pub presence: PeerFilePresenceFact,
    pub snapshot_row: Option<FileSnapshotRow>,
    pub last_seen: Option<SyncTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerFilePresenceFact {
    LiveFile(LiveFileFact),
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveFileFact {
    pub byte_size: u64,
    pub modified_time: SyncTimestamp,
    pub source_relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSnapshotRow {
    pub byte_size: Option<u64>,
    pub modified_time: Option<SyncTimestamp>,
    pub deleted_time: Option<SyncTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerFileClassification {
    pub peer_id: String,
    pub relative_path: String,
    pub state: PeerFileState,
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
pub struct FileOutcomeRequest {
    pub relative_path: String,
    pub peers: Vec<FileOutcomePeer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOutcomePeer {
    pub peer_id: String,
    pub role: FileOutcomePeerRole,
    pub classification: PeerFileState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOutcomePeerRole {
    Canon,
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOutcomeDecision {
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
    pub role: FileOutcomePeerRole,
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
pub enum FileOutcomesError {
    InvalidInput(String),
}

pub trait FileOutcomes: Send + Sync {
    /// Classifies one peer's supplied facts for one visible file path.
    ///
    /// The decision uses only the request's live-or-absent fact, optional
    /// snapshot row, and `last_seen` value for that peer. A live file always
    /// returns a live classification: it is new when no snapshot row is
    /// supplied, modified when the row has a non-null deletion time, unchanged
    /// when the row has no deletion time, matching byte size, and modification
    /// time within an inclusive 5-second tolerance, and modified when byte size
    /// differs or modification time differs by more than 5 seconds. The
    /// comparison is absolute, so up to and including 5 seconds earlier or
    /// later matches the snapshot row.
    ///
    /// An absent file returns a non-live classification. A row with a non-null
    /// deletion time returns `DeletedFile` with that exact deletion estimate. A
    /// row with no deletion time returns `AbsentUnconfirmed` and preserves the
    /// supplied `last_seen` value for later group decision making. No row
    /// returns `AbsentNoRowNoVote`.
    ///
    /// The method must return `FileOutcomesError::InvalidInput` instead of
    /// inventing metadata or producing a vote when the supplied facts cannot
    /// describe exactly one live-or-absent state for one peer and one path, or
    /// when required snapshot metadata for the selected comparison is missing.
    /// It has no external side effects and does not list, inspect, copy,
    /// delete, displace, update snapshots, or format output.
    fn classify_peer_file(
        &self,
        request: PeerFileClassificationRequest,
    ) -> Result<PeerFileClassification, FileOutcomesError>;

    /// Selects the group file outcome and one-path planner intents.
    ///
    /// The request must contain the classified state of every active peer for
    /// the one input path. With a canon peer, that peer's classification is the
    /// final outcome and non-canon peers cannot change it: a canon live file is
    /// the file outcome for every other active peer, while a canon state
    /// without a live file selects deletion for active peers that have a live
    /// file. Without a canon peer, only contributing peers vote. Subordinate
    /// peers never vote, but active subordinate peers can receive copy or
    /// displacement intents after the contributing outcome is selected.
    ///
    /// When all contributing peers that have a file are unchanged and matching,
    /// that unchanged file is the group outcome. Modified live-file votes and
    /// new live-file votes are each selected by newest modification time. Any
    /// live modification time within 5 seconds of the maximum is tied with the
    /// maximum; times more than 5 seconds behind lose. Among tied live votes,
    /// larger byte size wins. Tied live files with equal byte size are
    /// identical for planning even when their bytes differ, no copy is selected
    /// between identical peers, and a peer needing that file may copy from any
    /// identical source peer.
    ///
    /// When deleted votes and existing file votes both exist, the most recent
    /// deletion estimate is compared with the winning existing file
    /// modification time. A deletion estimate more than 5 seconds newer than
    /// the existing file selects deletion. An existing file whose modification
    /// time is not more than 5 seconds older than the deletion estimate wins,
    /// including exact ties. An absent-unconfirmed contributing peer
    /// contributes a deletion vote only when its `last_seen` is present and
    /// more than 5 seconds newer than the maximum live-file modification time;
    /// otherwise it does not vote and receives the file when an existing file
    /// wins.
    ///
    /// If every contributing peer is absent with no snapshot row, the group
    /// outcome is no file, no copy intent is returned, and active subordinate
    /// live files are selected for displacement. A peer that already has the
    /// winning byte size and a live modification time within 5 seconds of the
    /// winning modification time is not selected for copy. Deletion outcomes
    /// produce only deletion or displacement intents, never copy intents. Every
    /// returned outcome, source fact, peer decision fact, and intent is about
    /// the single input file path and has no execution side effect. Selected
    /// source facts and copy intents must preserve the exact source relative
    /// path supplied for the selected source peer.
    ///
    /// The method must return `FileOutcomesError::InvalidInput` instead of
    /// inventing metadata, fetching more state, or silently treating malformed
    /// role facts or classifications as votes when the supplied facts cannot
    /// describe one coherent decision for one visible file path.
    fn decide_file_outcome(
        &self,
        request: FileOutcomeRequest,
    ) -> Result<FileOutcomeDecision, FileOutcomesError>;
}
