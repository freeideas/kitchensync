#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunMissingPeerRootDecision {
    UrlUnreachable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunMissingPeerSnapshotDecision {
    CreateEmptyLocalTemporarySnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunLocalSnapshotCompletionDecision {
    KeepLocalTemporaryOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunPeerMutation {
    CreateDirectory,
    CreateFile,
    WriteFileContent,
    RenameEntry,
    DeleteDestinationFile,
    DisplaceDestinationToBak,
    SetModificationTime,
    UploadSnapshot,
    CleanBakStorage,
    CleanTmpStorage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryRunPeerMutationDecision {
    SkipPlannedAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DryRunOutputMarker {
    pub text: String,
}

pub trait DryRunPolicy: Send + Sync {
    /// Returns whether startup must establish connections to peer URLs.
    ///
    /// Dry-run startup still connects to every peer URL so normal reachability,
    /// authentication, fallback, and progress behavior can run. This policy
    /// does not establish the connection, choose fallback URLs, or decide the
    /// process exit code.
    fn should_connect_to_peer_urls(&self) -> bool;

    /// Returns whether startup may create a missing peer root directory.
    ///
    /// Dry-run never creates a missing peer root through either `file://` or
    /// `sftp://` URLs. Callers must check this before invoking the transport
    /// operation.
    fn may_create_missing_peer_root(&self) -> bool;

    /// Returns whether startup may create a missing peer root parent directory.
    ///
    /// Dry-run never creates missing peer root parents through either
    /// `file://` or `sftp://` URLs. Callers must check this before invoking the
    /// transport operation.
    fn may_create_missing_peer_root_parent(&self) -> bool;

    /// Decides how startup treats a selected URL whose peer root does not
    /// exist.
    ///
    /// In dry-run, a missing peer root makes that URL unreachable for this run
    /// instead of allowing root or parent directory creation. This is the
    /// specific startup case dry-run maps to unreachable; other read and
    /// connection errors remain for their owning callers to handle normally.
    fn decide_missing_peer_root(&self) -> DryRunMissingPeerRootDecision;

    /// Returns whether snapshot startup may run peer-side snapshot SWAP
    /// recovery before snapshot download.
    ///
    /// Dry-run skips peer-side `.kitchensync/SWAP/snapshot.db/` recovery. The
    /// caller must decide and call this policy before any transport mutation is
    /// attempted.
    fn may_run_peer_snapshot_swap_recovery(&self) -> bool;

    /// Returns whether an existing live peer snapshot should be downloaded.
    ///
    /// If `.kitchensync/snapshot.db` exists on an otherwise reachable peer,
    /// dry-run requires downloading that live file exactly as it is currently
    /// present on the peer. Snapshot download failures are not hidden by this
    /// policy and remain normal errors for the snapshot owner.
    fn should_download_existing_peer_snapshot(&self) -> bool;

    /// Decides how snapshot startup treats a reachable peer with no live
    /// snapshot.
    ///
    /// Dry-run requires creating a new empty local temporary snapshot database
    /// for that peer. This policy does not create the database and does not
    /// update snapshot rows.
    fn decide_missing_peer_snapshot(&self) -> DryRunMissingPeerSnapshotDecision;

    /// Returns whether traversal should list peer directories for decisions.
    ///
    /// Dry-run uses directory listings as normal planning input. Directory
    /// listing failures are not hidden by this policy and remain normal errors
    /// for the traversal owner.
    fn should_list_peer_directories(&self) -> bool;

    /// Returns whether traversal may run peer-side user-data SWAP recovery.
    ///
    /// Dry-run skips peer-side `.kitchensync/SWAP/` recovery for user data at
    /// each directory level. Callers must check this before invoking transport
    /// mutations.
    fn may_run_peer_user_data_swap_recovery(&self) -> bool;

    /// Returns whether traversal may update local temporary snapshot
    /// databases.
    ///
    /// Dry-run may update local temporary snapshot databases during traversal.
    /// The updates are local only; this policy does not write rows itself and
    /// does not permit uploading the result to peers.
    fn may_update_local_temporary_snapshot_databases(&self) -> bool;

    /// Returns whether queued copy work should be exercised.
    ///
    /// Dry-run uses the copy queue as in a normal run so copy planning,
    /// scheduling, and progress behavior are exercised. Destination-side
    /// writing is still forbidden by the peer-mutation guard.
    fn should_exercise_copy_queue(&self) -> bool;

    /// Returns whether a queued copy should acquire an active-copy slot.
    ///
    /// Dry-run queued work acquires active-copy slots as a normal run would.
    /// This policy does not own semaphores or enforce concurrency.
    fn should_acquire_active_copy_slot(&self) -> bool;

    /// Returns whether a queued copy should read the source file.
    ///
    /// Dry-run queued work reads source files as a normal run would. Source
    /// read failures are not hidden by this policy and remain normal errors for
    /// the copy owner.
    fn should_read_copy_source_file(&self) -> bool;

    /// Returns whether queued copy retry limits still apply.
    ///
    /// Dry-run tracks per-copy try counts and applies the `--retries-copy`
    /// total try limit as a normal run would. Skipped peer mutations are not
    /// transport failures and must not be retried as peer I/O errors.
    fn should_apply_copy_retry_limit(&self) -> bool;

    /// Returns whether `C` progress events should be emitted for copy work.
    ///
    /// Dry-run emits `C` progress events in the same cases as a normal run.
    /// The event describes the exercised copy work; it does not mean
    /// destination-side file content was written.
    fn should_emit_copy_progress_events(&self) -> bool;

    /// Returns whether failed-copy `X` progress events should be emitted.
    ///
    /// Dry-run emits failed-copy `X` progress events in the same cases as a
    /// normal run. This policy does not format or print stdout.
    fn should_emit_failed_copy_progress_events(&self) -> bool;

    /// Returns whether planned removal or BAK displacement `X` progress events
    /// should be emitted.
    ///
    /// When dry-run skips a planned deletion or BAK displacement, the matching
    /// `X` progress event is still allowed in the same case where a normal run
    /// would emit it. The event describes the plan; it does not mean the peer
    /// entry was renamed or removed.
    fn should_emit_planned_removal_or_displacement_events(&self) -> bool;

    /// Decides whether a peer-side mutation may be invoked.
    ///
    /// For `file://` and `sftp://` peers, dry-run always returns a skipped
    /// planned action for peer directory creation, including TMP, SWAP, BAK,
    /// and destination directories; peer file creation; destination content
    /// writes; renames; destination deletes; BAK displacement; modification
    /// time updates; snapshot upload; BAK cleanup; and TMP cleanup. Callers
    /// must ask before calling transport. A skipped dry-run action is not a
    /// transport failure and must not be retried as peer I/O.
    fn decide_peer_mutation(&self, mutation: DryRunPeerMutation) -> DryRunPeerMutationDecision;

    /// Decides what completion may do with updated local temporary snapshots.
    ///
    /// Dry-run may leave local temporary snapshot databases updated, but must
    /// not upload them to any peer at the end of the run, including
    /// subordinate peers. Snapshot upload is also denied by the peer-mutation
    /// guard.
    fn decide_local_snapshot_completion(&self) -> DryRunLocalSnapshotCompletionDecision;

    /// Returns the dry-run stdout marker event.
    ///
    /// A dry-run execution must cause stdout to contain the exact phrase
    /// `dry run` at least once. This method returns structured output data
    /// only; final formatting and printing belong to the output owner.
    fn output_marker(&self) -> DryRunOutputMarker;
}
