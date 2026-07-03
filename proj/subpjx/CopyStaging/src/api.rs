use std::time::SystemTime;

use peertransportsurface::{ConnectedPeerRoot, PeerTransportError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingRunMode {
    Normal,
    DryRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingVerbosity {
    Error,
    Info,
    Debug,
    Trace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyStagingRunOptions {
    pub mode: CopyStagingRunMode,
    pub max_copies: u64,
    pub retries_copy: u64,
    pub keep_bak_days: u64,
    pub keep_tmp_days: u64,
    pub verbosity: CopyStagingVerbosity,
}

#[derive(Clone)]
pub struct CopyStagingPeer {
    pub peer_index: usize,
    pub peer_url: String,
    pub root: ConnectedPeerRoot,
}

#[derive(Clone)]
pub struct CopyStagingCopyRequest {
    pub options: CopyStagingRunOptions,
    pub source_peer: CopyStagingPeer,
    pub destination_peer: CopyStagingPeer,
    pub source_path: String,
    pub destination_path: String,
    pub relative_path: String,
    pub winning_mod_time: SystemTime,
    pub winning_byte_size: i64,
}

#[derive(Clone)]
pub struct CopyStagingDirectoryRequest {
    pub options: CopyStagingRunOptions,
    pub peer: CopyStagingPeer,
    pub directory_relative_path: Option<String>,
}

#[derive(Clone)]
pub struct CopyStagingDisplacementRequest {
    pub options: CopyStagingRunOptions,
    pub peer: CopyStagingPeer,
    pub relative_path: String,
    pub is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyStagingCopyResult {
    pub destination_peer_index: usize,
    pub destination_peer_url: String,
    pub relative_path: String,
    pub status: CopyStagingCopyStatus,
    pub attempts: u64,
    pub output_lines: Vec<String>,
    pub diagnostics: Vec<CopyStagingDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingCopyStatus {
    Completed,
    PlannedDryRun,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyStagingSwapRecoveryResult {
    pub peer_index: usize,
    pub directory_relative_path: Option<String>,
    pub status: CopyStagingSwapRecoveryStatus,
    pub output_lines: Vec<String>,
    pub diagnostics: Vec<CopyStagingDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingSwapRecoveryStatus {
    Recovered,
    SkippedDryRun,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyStagingDisplacementResult {
    pub peer_index: usize,
    pub peer_url: String,
    pub relative_path: String,
    pub status: CopyStagingDisplacementStatus,
    pub output_lines: Vec<String>,
    pub diagnostics: Vec<CopyStagingDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingDisplacementStatus {
    Displaced,
    PlannedDryRun,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyStagingCleanupResult {
    pub peer_index: usize,
    pub directory_relative_path: Option<String>,
    pub status: CopyStagingCleanupStatus,
    pub output_lines: Vec<String>,
    pub diagnostics: Vec<CopyStagingDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingCleanupStatus {
    Completed,
    SkippedDryRun,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyStagingDiagnostic {
    pub level: CopyStagingDiagnosticLevel,
    pub peer_index: usize,
    pub peer_url: String,
    pub relative_path: Option<String>,
    pub kind: CopyStagingDiagnosticKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingDiagnosticLevel {
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingDiagnosticKind {
    TransferFailed {
        phase: CopyStagingFailurePhase,
        transport_error: Option<PeerTransportError>,
    },
    CopyTriesExhausted,
    SwapRecoveryFailed {
        transport_error: Option<PeerTransportError>,
    },
    DisplacementFailed {
        transport_error: Option<PeerTransportError>,
    },
    CleanupFailed {
        transport_error: Option<PeerTransportError>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStagingFailurePhase {
    ReadSource,
    WriteSwapNew,
    MoveExistingToSwapOld,
    RenameFinal,
    SetModTime,
    ArchiveOld,
    Cleanup,
}

pub trait CopyStaging: Send + Sync {
    /// Performs one queued destination file copy chosen by traversal. The copy
    /// is exactly one source peer path to one destination peer path and keeps
    /// its own try count. `retries_copy` is the maximum number of total tries,
    /// including the first try. A failure before SWAP `old` exists consumes
    /// only this copy's try, and if tries remain this copy waits behind other
    /// queued work before trying again. When no tries remain, the result is
    /// failed and contains one error-level exhausted-tries diagnostic.
    ///
    /// Before reading or writing file content, each try acquires one global
    /// copy slot. No more than `max_copies` transfers hold slots at the same
    /// time, and the limit is global across all source and destination
    /// schemes. Directory listing, snapshot work, directory creation, BAK
    /// cleanup, TMP cleanup, and SWAP cleanup do not consume copy slots. At
    /// trace verbosity, each acquire and release contributes a plain output
    /// line `copy-slots active=<n>/<max>` with `active` measured after the
    /// event.
    ///
    /// A successful normal copy streams the selected source bytes with bounded
    /// buffer storage, makes those bytes live at the destination path, and sets
    /// the destination modification time to `winning_mod_time`. For an
    /// existing destination file, replacement never depends on rename over an
    /// existing path: SWAP `new` is written before the live destination is
    /// moved, the live destination is moved to SWAP `old` before `new` is
    /// renamed live, and the replaced file is moved from `old` to BAK after
    /// the replacement is live. If setting the modification time fails after
    /// the copied file is live, the live file is not undone; the result is
    /// failed and contains an error-level `SetModTime` diagnostic so a later
    /// run can rediscover the mismatch.
    ///
    /// Failures before SWAP `old` exists remove this transfer's SWAP `new`
    /// staging when possible and then follow the retry rules. Once SWAP `old`
    /// exists, failures leave recoverable SWAP state for a later normal run
    /// and return an error-level diagnostic. Archive-old failure after the
    /// replacement is live leaves SWAP `old` for later recovery and returns an
    /// error-level diagnostic. Diagnostics for transfer failures identify the
    /// relative path, destination peer URL, failed phase, and transport error
    /// category when one is available.
    ///
    /// At info, debug, and trace verbosity, a successful or planned copy
    /// contributes one `C <relpath>` output line for the copied relative path,
    /// regardless of how many peers receive that path. At error verbosity, copy
    /// progress is suppressed. In dry-run mode, source reads, slot accounting,
    /// retry accounting, and progress output still happen, but destination
    /// peer writes, SWAP, BAK, TMP, and modification-time writes are skipped
    /// and a planned dry-run result represents success for dry-run decision
    /// flow only.
    fn copy_file(&self, request: CopyStagingCopyRequest) -> CopyStagingCopyResult;

    /// Recovers user-file SWAP state for one peer at one directory before
    /// traversal lists that directory's live user entries for decisions. In a
    /// normal run, each direct child of `.kitchensync/SWAP/` at that directory
    /// level is recovered for the corresponding target basename in the same
    /// parent directory. The recovery cases are idempotent: delete stray `new`
    /// when live already has the target and `old` is absent; move `new` live
    /// when only `new` exists and live is missing; move `old` back live when
    /// only `old` exists and live is missing; when `old` and live exist,
    /// delete `new` if present, move `old` to BAK, and remove the empty SWAP
    /// directory; and when `old` and `new` exist without live, move `new` live,
    /// move `old` to BAK, and remove the empty SWAP directory.
    ///
    /// If recovery fails, the result is failed and contains an error-level
    /// diagnostic so traversal can skip sync decisions for this peer's current
    /// directory subtree and leave its snapshot rows unchanged for that
    /// subtree. In dry-run mode, peer-side user-file SWAP recovery is skipped
    /// and existing peer SWAP state is left untouched.
    fn recover_user_swap(
        &self,
        request: CopyStagingDirectoryRequest,
    ) -> CopyStagingSwapRecoveryResult;

    /// Moves one selected live file or directory to BAK as inline traversal
    /// work. Before the rename in a normal run, this operation creates
    /// `<parent>/.kitchensync/BAK/<timestamp>/` and any missing parents, with
    /// the timestamp formatted by the shared format rules. A displaced file is
    /// moved to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. A
    /// displaced directory is moved to the same shape as one directory tree.
    /// The operation does not acquire a copy slot.
    ///
    /// If displacement fails, the original live path remains in place, the
    /// result is failed, and an error-level diagnostic is returned; callers
    /// must not record a snapshot deletion update for that peer path. At info,
    /// debug, and trace verbosity, a successful or planned displacement
    /// contributes one `X <relpath>` output line for the deleted relative path,
    /// regardless of how many peers displace that path. At error verbosity,
    /// delete progress is suppressed. In dry-run mode, the peer write and BAK
    /// creation are skipped and a planned dry-run result represents success
    /// for dry-run decision flow only.
    fn displace_to_bak(
        &self,
        request: CopyStagingDisplacementRequest,
    ) -> CopyStagingDisplacementResult;

    /// Runs BAK and TMP cleanup for one peer at one visited directory level in
    /// a normal run. Cleanup checks only that directory's `.kitchensync/`
    /// metadata directory, removes `.kitchensync/BAK/<timestamp>/` directories
    /// older than `keep_bak_days`, and removes `.kitchensync/TMP/<timestamp>/`
    /// directories older than `keep_tmp_days`. Age is determined from the
    /// timestamp path segment parsed by the shared format rules, not from
    /// filesystem metadata. Timestamp directories that are not older than the
    /// configured retention are left in place, malformed timestamp directories
    /// are not age-deleted, and `.kitchensync/SWAP/` directories are never
    /// removed by age.
    ///
    /// Cleanup emits no copy or delete progress lines and never consumes a
    /// copy slot. If cleanup fails, the result is failed and contains an
    /// error-level diagnostic. In dry-run mode, peer-side BAK and TMP cleanup
    /// is skipped and existing peer metadata entries are left untouched.
    fn cleanup_metadata(
        &self,
        request: CopyStagingDirectoryRequest,
    ) -> CopyStagingCleanupResult;
}
