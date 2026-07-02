use std::sync::Arc;

pub type QueueRunnerTransferOperation =
    Arc<dyn Fn(QueueRunnerCopyWork, u32) -> QueueRunnerTransferResult + Send + Sync>;

pub type QueueRunnerEventSink = Arc<dyn Fn(QueueRunnerEvent) + Send + Sync>;

#[derive(Clone)]
pub struct QueueRunnerRunConfig {
    pub max_active_copies: Option<u32>,
    pub max_total_tries_per_copy: Option<u32>,
    pub transfer_operation: QueueRunnerTransferOperation,
    pub event_sink: QueueRunnerEventSink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueRunnerCopyWork {
    pub copy_id: QueueRunnerCopyId,
    pub source_scheme: QueueRunnerPeerScheme,
    pub destination_scheme: QueueRunnerPeerScheme,
    pub user_path: String,
    pub destination_peer_identity: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueueRunnerCopyId {
    pub value: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueRunnerPeerScheme {
    File,
    Sftp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueRunnerTransferResult {
    Success,
    SkipForRun(QueueRunnerTransferFailure),
    Failure(QueueRunnerTransferFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueRunnerTransferFailure {
    pub phase: QueueRunnerTransferPhase,
    pub transport_error: Option<QueueRunnerTransportErrorCategory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueRunnerTransferPhase {
    ReadSource,
    WriteSwapNew,
    MoveExistingToSwapOld,
    RenameFinal,
    SetModTime,
    ArchiveOld,
    Cleanup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueRunnerTransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueRunnerEvent {
    CopyStart(QueueRunnerCopyAttempt),
    CopySlotAcquire(QueueRunnerSlotEvent),
    CopySlotRelease(QueueRunnerSlotEvent),
    TransferSuccess(QueueRunnerCopyAttempt),
    TransferSkip(QueueRunnerTransferEvent),
    TransferFailure(QueueRunnerTransferEvent),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueRunnerSlotEvent {
    pub copy: QueueRunnerCopyAttempt,
    pub active_after_event: u32,
    pub max_active_copies: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueRunnerCopyAttempt {
    pub copy_id: QueueRunnerCopyId,
    pub user_path: String,
    pub destination_peer_identity: String,
    pub try_number: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueRunnerTransferEvent {
    pub copy: QueueRunnerCopyAttempt,
    pub failure: QueueRunnerTransferFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueRunnerRunResult {
    pub copies: Vec<QueueRunnerCopyResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueRunnerCopyResult {
    pub copy_id: QueueRunnerCopyId,
    pub user_path: String,
    pub destination_peer_identity: String,
    pub total_tries: u32,
    pub outcome: QueueRunnerCopyOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueRunnerCopyOutcome {
    Succeeded,
    SkippedForRun,
    FailedAfterTryLimit,
}

pub trait QueueRunner: Send + Sync {
    /// Starts one run-scoped queue using the supplied active-copy limit,
    /// per-copy total try limit, staged-transfer operation, and event sink.
    ///
    /// `None` for `max_active_copies` means the run uses the default global
    /// maximum of `10`; `Some(N)` means the run uses `N`. `None` for
    /// `max_total_tries_per_copy` means the run uses the default total try
    /// limit of `3`; `Some(N)` means the run allows at most `N` total tries
    /// for each queued copy, including the first try. The maximum active copy
    /// count is one global pool shared by every supported source and
    /// destination scheme combination: file to file, file to SFTP, SFTP to
    /// file, and SFTP to SFTP. The run must not create a lower per-peer,
    /// per-host, or per-connection limit. Directory listing, snapshot
    /// download, snapshot upload, directory creation, and BAK, TMP, or SWAP
    /// cleanup are outside this slot pool.
    fn start_run(&self, config: QueueRunnerRunConfig);

    /// Enqueues one already-eligible file copy for the active run.
    ///
    /// The copy becomes eligible to start as soon as it is queued and a global
    /// copy slot is available; QueueRunner must not wait for traversal to
    /// close the queue before starting eligible work. Each enqueued copy tracks
    /// its own try count independently, so a failure for one copy never
    /// increments, resets, or caps the tries of another copy. Copy try
    /// accounting is scheme-independent: local, SFTP, and mixed local/SFTP
    /// copies use the same total try limit, requeue-behind rule, and
    /// no-requeue-after-limit rule. The caller supplies copy work that is
    /// already eligible; this operation does not decide sync outcomes, choose
    /// peers, parse URLs, authenticate, list directories, update snapshots, or
    /// format stdout.
    fn enqueue_copy(&self, copy: QueueRunnerCopyWork);

    /// Closes the active run's queue and waits for all queued work to settle.
    ///
    /// Closing means traversal will not enqueue more copies. Draining finishes
    /// only after every queued copy has succeeded, been skipped for this run,
    /// or reached its copy try limit. Every staged-transfer try emits one copy
    /// start event for the attempt, acquires one global slot just before the
    /// transfer operation begins, emits a slot acquire event with the active
    /// count after acquisition and the global maximum, calls the staged-transfer
    /// operation, emits the matching transfer success, transfer skip, or
    /// transfer failure event from that result, and releases exactly that slot
    /// once after the transfer operation has returned. The release event
    /// reports the active count after release and the same global maximum. On
    /// success, the copy is recorded complete and is not queued again. On skip,
    /// the copy is recorded skipped for the run and is not queued again. On
    /// failure before the copy reaches its total try limit, only that copy's
    /// try count is incremented, the same copy is placed behind other queued
    /// copy work, the slot for the failed try is released, and other queued
    /// work continues in the same run. On failure at the total try limit, the
    /// copy is recorded failed for the run and is not requeued again in that
    /// run.
    fn close_and_drain(&self) -> QueueRunnerRunResult;
}
