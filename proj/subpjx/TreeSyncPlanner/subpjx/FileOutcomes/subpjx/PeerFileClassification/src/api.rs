#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerFileTimestamp {
    pub unix_seconds: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerFileClassificationRequest {
    pub peer_id: String,
    pub relative_path: String,
    pub presence: PeerFilePresenceFact,
    pub snapshot_row: Option<PeerFileSnapshotRow>,
    pub last_seen: Option<PeerFileTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerFilePresenceFact {
    LiveFile(PeerLiveFileFact),
    AbsentFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerLiveFileFact {
    pub byte_size: u64,
    pub modified_time: PeerFileTimestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerFileSnapshotRow {
    pub byte_size: Option<u64>,
    pub modified_time: Option<PeerFileTimestamp>,
    pub deleted_time: Option<PeerFileTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerFileClassificationResult {
    pub peer_id: String,
    pub relative_path: String,
    pub state: PeerFileClassificationState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerFileClassificationState {
    UnchangedLiveFile(ClassifiedPeerLiveFile),
    ModifiedLiveFile(ClassifiedPeerLiveFile),
    NewLiveFile(ClassifiedPeerLiveFile),
    DeletedFile {
        deletion_estimate: PeerFileTimestamp,
    },
    AbsentUnconfirmed {
        last_seen: Option<PeerFileTimestamp>,
    },
    AbsentNoRowNoVote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedPeerLiveFile {
    pub byte_size: u64,
    pub modified_time: PeerFileTimestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerFileClassificationError {
    InvalidInput(String),
}

pub trait PeerFileClassification: Send + Sync {
    /// Classifies one peer's supplied facts for one visible file path.
    ///
    /// The operation is pure decision logic. It uses only the supplied live or
    /// absent fact, optional snapshot row, and optional `last_seen` value for
    /// the peer and path in the request. It must not inspect files, read or
    /// write snapshots, access transports, compare file bytes, produce copy or
    /// displacement intents, format output, or mutate external state. Repeated
    /// calls with the same request return the same classification or the same
    /// invalid-input error.
    ///
    /// A live file always returns a live classification. It is new when no
    /// snapshot row is supplied. It is modified when the snapshot row has a
    /// non-null `deleted_time`; the prior deletion marker does not make the
    /// live file deleted. With a snapshot row whose `deleted_time` is null, the
    /// live file is unchanged only when its byte size matches the snapshot byte
    /// size and its modification time is within an inclusive 5-second
    /// tolerance of the snapshot modification time. The comparison is absolute:
    /// up to and including 5 seconds earlier or later matches. A different byte
    /// size, or a modification time more than 5 seconds earlier or later, is
    /// modified.
    ///
    /// An absent file always returns a non-live classification. With no
    /// snapshot row it returns `AbsentNoRowNoVote`. With a snapshot row whose
    /// `deleted_time` is non-null, it returns `DeletedFile` carrying that exact
    /// value as the deletion estimate. With a snapshot row whose `deleted_time`
    /// is null, it returns `AbsentUnconfirmed` and preserves the supplied
    /// optional `last_seen` value for later group outcome selection.
    ///
    /// The returned live classifications preserve the live byte size and live
    /// modification time. The operation returns
    /// `PeerFileClassificationError::InvalidInput` instead of inventing
    /// metadata or silently producing a vote when the supplied facts do not
    /// describe exactly one live-or-absent state for one peer and one file
    /// path, or when a live comparison against a non-deleted snapshot row is
    /// missing the snapshot byte size or snapshot modification time.
    fn classify_peer_file(
        &self,
        request: PeerFileClassificationRequest,
    ) -> Result<PeerFileClassificationResult, PeerFileClassificationError>;
}
