use std::any::Any;
use std::num::NonZeroU32;
use std::sync::Arc;

pub type CopyQueueEventSink = Arc<dyn Fn(CopyQueueEvent) + Send + Sync + 'static>;

#[derive(Clone)]
pub struct ConnectedPeerHandle {
    pub identity: String,
    pub winning_url: String,
    pub scheme: PeerScheme,
    pub handle: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerScheme {
    File,
    Sftp,
}

pub struct CopyQueueRunRequest {
    pub max_active_copies: Option<NonZeroU32>,
    pub max_total_tries_per_copy: Option<NonZeroU32>,
    pub peers: Vec<ConnectedPeerHandle>,
    pub mutation_policy: CopyMutationPolicy,
    pub event_sink: CopyQueueEventSink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyMutationPolicy {
    Normal,
    DryRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CopyQueueRunId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedCopy {
    pub source_peer_identity: String,
    pub source_relative_file_path: String,
    pub destination_peer_identity: String,
    pub destination_relative_file_path: String,
    pub report_relative_path: String,
    pub winning_mod_time: String,
    pub winning_byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyQueueEvent {
    CopyStart {
        relpath: String,
        source_peer_identity: String,
        destination_peer_identity: String,
        try_number: u32,
    },
    CopySlotAcquire {
        active: u32,
        max: u32,
    },
    CopySlotRelease {
        active: u32,
        max: u32,
    },
    TransferSuccess {
        relpath: String,
        destination_peer_identity: String,
    },
    TransferSkip {
        relpath: String,
        destination_peer_identity: String,
        phase: CopyFailurePhase,
        transport_error: Option<TransportErrorCategory>,
    },
    TransferFailure {
        relpath: String,
        destination_peer_identity: String,
        phase: CopyFailurePhase,
        transport_error: Option<TransportErrorCategory>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyQueueDrainResult {
    pub results: Vec<QueuedCopyResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedCopyResult {
    pub copy: QueuedCopy,
    pub total_tries: u32,
    pub outcome: QueuedCopyOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueuedCopyOutcome {
    Succeeded,
    SkippedForRun {
        phase: CopyFailurePhase,
        transport_error: Option<TransportErrorCategory>,
    },
    FailedTryLimit {
        phase: CopyFailurePhase,
        transport_error: Option<TransportErrorCategory>,
        installation_state: CopyInstallationState,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyInstallationState {
    NotInstalled,
    OriginalDestinationStillInPlace,
    SwapOldLeftForRecovery,
    FinalDestinationInstalled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyFailurePhase {
    ReadSource,
    WriteSwapNew,
    MoveExistingToSwapOld,
    RenameFinal,
    SetModTime,
    ArchiveOld,
    Cleanup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyQueueError {
    UnknownRun,
    QueueClosed,
    DuplicatePeerIdentity(String),
    UnknownSourcePeer(String),
    UnknownDestinationPeer(String),
}

pub trait CopyQueue: Send + Sync {
    /// Opens one run-scoped copy queue and returns the token used by later
    /// enqueue and drain calls.
    ///
    /// When `max_active_copies` is `None`, the queue uses a global maximum of
    /// `10`; otherwise it uses the supplied positive value. When
    /// `max_total_tries_per_copy` is `None`, the queue uses `3`; otherwise it
    /// uses the supplied positive value. The total try limit is per queued copy
    /// and includes the first try. The connected peer handles are already
    /// established for their winning URLs and later copy work must use only
    /// those handles, without reconnecting or selecting fallback URLs. The dry
    /// run mutation policy is applied before any operation that might acquire a
    /// copy slot, read source content, or mutate the destination side; this
    /// trait does not decide dry-run semantics itself. The event sink receives
    /// structured events for starts, slot changes, successes, skips, and
    /// failures; stdout formatting is outside this trait. Duplicate peer
    /// identities are rejected before the run is opened.
    fn open_run(&self, request: CopyQueueRunRequest) -> Result<CopyQueueRunId, CopyQueueError>;

    /// Adds one already-eligible file copy to an open run queue.
    ///
    /// Enqueued work becomes eligible immediately and may start before
    /// traversal has finished scanning the whole tree, subject only to the
    /// run's global active-copy limit and available queued work. The queue
    /// must not impose any lower per-peer, per-host, or per-connection copy
    /// limit. The source and destination peer identities must name peers from
    /// the run request. The relative source and destination paths are already
    /// selected by the caller; this trait does not decide winners, canon
    /// status, excludes, displacements, or traversal. Enqueue after the run has
    /// been closed for draining is rejected and does not add work.
    fn enqueue(&self, run_id: CopyQueueRunId, copy: QueuedCopy) -> Result<(), CopyQueueError>;

    /// Closes one run queue and waits until all accepted work has reached a
    /// terminal run outcome.
    ///
    /// Closing means traversal will enqueue no more copies. Draining returns
    /// only after every queued copy has succeeded, has been skipped for this
    /// run, or has reached its per-copy total try limit. Every active try holds
    /// exactly one global copy slot from just before the transfer try begins
    /// until after all cleanup required for that try and the matching slot
    /// release event have completed. The limit is shared across file-to-file,
    /// file-to-sftp, sftp-to-file, and sftp-to-sftp copies; directory listing,
    /// snapshot transfer, directory creation, and BAK, TMP, or SWAP cleanup
    /// outside a copy try do not count as active file copies.
    ///
    /// For each normal transfer, the destination basename is percent-encoded
    /// when needed so the encoded value is one path segment on every supported
    /// transport. Pre-transfer recovery of an existing destination SWAP
    /// directory is outside this trait and must already have been handled
    /// before CopyQueue writes a replacement for that encoded basename.
    /// Replacement content must be written only to SWAP `new`; any existing
    /// destination file must move to SWAP `old` before SWAP `new` moves to the
    /// final path; the final destination modification time must be set to the
    /// winning modification time; any SWAP `old` must be archived below the
    /// nearby BAK timestamp directory using a fresh process-local timestamp
    /// string requested from the snapshot child for that archive path; and
    /// successful transfers must remove their empty SWAP directories. A
    /// destination that had no existing file creates no BAK entry. Active
    /// transfer I/O must stream with bounded buffering whose memory use is
    /// independent of source file size.
    ///
    /// Failed copies keep independent try counts. A retryable failure before
    /// the try limit moves only that copy behind other queued work and lets
    /// other work continue. A copy that reaches its try limit is not requeued
    /// in the same run. If failure occurs before SWAP `old` exists, SWAP `new`
    /// is deleted when possible before the slot is released. If moving the
    /// existing destination to SWAP `old` fails, the original destination stays
    /// in place, SWAP `new` is deleted when possible, the phase is reported as
    /// `MoveExistingToSwapOld`, and that copy is skipped for the rest of the
    /// run instead of retried. A failure after SWAP `old` exists leaves
    /// peer-visible SWAP state for later recovery. If setting modification time
    /// or archiving SWAP `old` fails after the replacement is installed, the
    /// replacement is not undone and the result reports that final destination
    /// state to the caller. Failure events include the relative path,
    /// destination peer identity, transport error category when available, and
    /// exactly one failed phase.
    fn close_and_drain(
        &self,
        run_id: CopyQueueRunId,
    ) -> Result<CopyQueueDrainResult, CopyQueueError>;
}
