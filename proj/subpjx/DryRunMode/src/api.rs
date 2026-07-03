#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunModePeerScheme {
    File,
    Sftp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunModeRootState {
    Exists,
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunModeStartupRootDecision {
    UseExistingRoot,
    FailCandidateWithoutCreatingRoot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunModeSnapshotDownloadOutcome {
    Found,
    NotFound,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunModeSnapshotStartupDecision {
    UseLiveSnapshotAsLocalTemporary,
    CreateEmptyLocalTemporarySnapshot,
    ExcludePeerWithErrorDiagnostic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunModeWorkKind {
    ConnectToExistingRoot,
    ListDirectory,
    StatPath,
    DownloadSnapshot,
    ReadSourceFile,
    CreateOrUpdateLocalTemporarySnapshot,
    CreatePeerDirectory,
    CreatePeerMetadataDirectory,
    WritePeerFile,
    RenamePeerEntry,
    DeletePeerEntry,
    DisplacePeerEntryToBak,
    SetPeerModificationTime,
    RecoverPeerSnapshotSwap,
    RecoverPeerUserFileSwap,
    CleanPeerBakTmp,
    UploadPeerSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunModeWorkDecision {
    AllowPeerRead,
    AllowLocalWorkingWrite,
    SuppressPeerWritePlannedSuccess,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DryRunModeCopyWorkPolicy {
    pub acquire_copy_slots: bool,
    pub read_sources: bool,
    pub apply_normal_retry_limit: bool,
    pub emit_copy_progress: bool,
    pub emit_delete_progress: bool,
}

pub trait DryRunMode: Send + Sync {
    /// Returns the exact stdout line that begins every dry-run sync. The
    /// coordinator must emit this line before any progress line or `sync
    /// complete` line, including dry runs configured at error verbosity.
    /// Calling this operation repeatedly is idempotent and cannot change any
    /// peer or local working state.
    fn dry_run_output_line(&self) -> String;

    /// Returns the startup root decision for one candidate peer URL in
    /// dry-run mode. Existing `file://` and `sftp://` roots remain eligible
    /// for normal reachable-peer startup and later operations through the
    /// winning URL. Missing roots fail only that candidate for the run, and
    /// the caller must not create the peer root or any missing parent
    /// directory. For SFTP candidates, connection state may already have been
    /// established before the remote root existence check. The decision is
    /// deterministic for the supplied scheme and root state.
    fn startup_root_decision(
        &self,
        scheme: DryRunModePeerScheme,
        root_state: DryRunModeRootState,
    ) -> DryRunModeStartupRootDecision;

    /// Returns the snapshot startup decision after a dry-run live snapshot
    /// download attempt. Peer-side snapshot SWAP recovery is skipped before
    /// this decision is reached. A found live `.kitchensync/snapshot.db` is
    /// used as-is as the local temporary snapshot database input. A not-found
    /// live snapshot creates only a new empty local temporary database and
    /// does not make the peer unreachable. Any other download failure excludes
    /// that peer from the reachable set for the run and requires the normal
    /// error-level snapshot startup diagnostic.
    fn snapshot_startup_decision(
        &self,
        outcome: DryRunModeSnapshotDownloadOutcome,
    ) -> DryRunModeSnapshotStartupDecision;

    /// Classifies one unit of dry-run work. Peer reads are allowed and still
    /// surface real read, connection, listing, stat, or snapshot download
    /// failures through the normal caller paths. Local temporary snapshot
    /// database creation and updates are allowed as working state only. Peer
    /// writes are suppressed for dry-run decision flow and return planned
    /// success to the caller; suppressed peer writes must not invoke the
    /// underlying peer transport and therefore must not report transport
    /// errors from the skipped write phase. This classification is idempotent
    /// and never changes peer or local state by itself.
    fn classify_work(&self, work: DryRunModeWorkKind) -> DryRunModeWorkDecision;

    /// Returns the dry-run copy policy. Queued copy work still acquires the
    /// same global copy slots subject to `--max-copies`, reads source files,
    /// and applies the configured `--retries-copy` total-try limit exactly as
    /// a normal run. Source read failures are real copy try failures and are
    /// retried or exhausted through the normal copy rules. Copy and delete
    /// progress lines are still emitted under the same verbosity settings as
    /// a normal run, while destination peer mutations remain governed by
    /// `classify_work`.
    fn copy_work_policy(&self) -> DryRunModeCopyWorkPolicy;
}
