#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdoutVerbosity {
    Error,
    Info,
    Debug,
    Trace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StdoutErrorDiagnostic {
    pub kind: StdoutErrorKind,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdoutErrorKind {
    ArgumentError,
    NoSnapshotsAndNoCanon,
    UnreachablePeer,
    DirectoryListingFailure,
    CanonPeerUnreachable,
    FewerThanTwoReachablePeers,
    NoContributingPeerReachable,
    TransferFailureBeforeSwapOld,
    TransferFailureAfterSwapOld,
    ArchiveOldFailure,
    DisplacementFailure,
    TmpOrSwapStagingFailure,
    SetModTimeFailure,
    SnapshotUploadFailureBeforeSwapOld,
    SnapshotUploadFailureAfterSwapOld,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StdoutFailedFileTransferDiagnostic {
    pub relpath: String,
    pub destination_peer_url: String,
    pub phase: StdoutFileTransferPhase,
    pub transport_error_category: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdoutFileTransferPhase {
    ReadSource,
    WriteSwapNew,
    MoveExistingToSwapOld,
    RenameFinal,
    SetModTime,
    ArchiveOld,
    Cleanup,
}

pub trait StdoutReporter: Send + Sync {
    /// Writes a non-help argument validation failure to stdout.
    ///
    /// The error message is written before the supplied help text, and the help
    /// text is used exactly as supplied by the caller. This is an error-level
    /// report, so it is visible at every verbosity. The method never writes to
    /// stderr and never inspects whether stdout is a terminal. Calls are not
    /// idempotent: each call emits another complete set of stdout lines in the
    /// order the call is made.
    fn report_argument_validation_failure(
        &self,
        verbosity: StdoutVerbosity,
        error_message: String,
        help_text: String,
    );

    /// Writes the exact first-sync decision failure line to stdout.
    ///
    /// The emitted line is exactly
    /// `First sync? Mark the authoritative peer with a leading +`. This is an
    /// error-level report, so it is visible at every verbosity. The method does
    /// not decide whether the peer set is valid and does not select an exit
    /// code. It never writes to stderr, never emits terminal-dependent
    /// formatting, and is not idempotent.
    fn report_first_sync_requires_authoritative_peer(&self, verbosity: StdoutVerbosity);

    /// Writes the exact no-contributing-peer decision failure line to stdout.
    ///
    /// The emitted line is exactly
    /// `No contributing peer reachable - cannot make sync decisions`. This is
    /// an error-level report, so it is visible at every verbosity. The method
    /// does not decide peer reachability, contribution status, or process exit
    /// code. It never writes to stderr, never emits terminal-dependent
    /// formatting, and is not idempotent.
    fn report_no_contributing_peer_reachable(&self, verbosity: StdoutVerbosity);

    /// Writes one error-level diagnostic for an already-decided sync error.
    ///
    /// The diagnostic kind must be one of the error conditions named by the
    /// product sync error specification, and caller-supplied details are
    /// formatted as plain stdout text. Error diagnostics are visible at every
    /// verbosity. The method never retries operations, changes sync state,
    /// selects an exit code, writes to stderr, or emits terminal-dependent
    /// formatting. Calls are emitted in caller order and are not idempotent.
    fn report_error_diagnostic(
        &self,
        verbosity: StdoutVerbosity,
        diagnostic: StdoutErrorDiagnostic,
    );

    /// Writes one diagnostic for an already-failed file transfer.
    ///
    /// The emitted diagnostic must include the slash-separated relative path,
    /// destination peer URL, failed phase label, and transport error category
    /// when the category is present. The phase is rendered as one of
    /// `read_source`, `write_swap_new`, `move_existing_to_swap_old`,
    /// `rename_final`, `set_mod_time`, `archive_old`, or `cleanup`. This is an
    /// error-level report, so it is visible at every verbosity. The method does
    /// not retry the transfer, classify transport errors, write to stderr, or
    /// emit terminal-dependent formatting. Calls are emitted in caller order
    /// and are not idempotent.
    fn report_failed_file_transfer(
        &self,
        verbosity: StdoutVerbosity,
        diagnostic: StdoutFailedFileTransferDiagnostic,
    );

    /// Writes one logical copy progress line when info-level output is visible.
    ///
    /// At `Info`, `Debug`, and `Trace` verbosity, the emitted line is exactly
    /// `C <relpath>`, with one space after `C` and the slash-separated relative
    /// path supplied by the caller. At `Error` verbosity, no line is emitted.
    /// One logical copied path is reported by one caller invocation regardless
    /// of how many destination peers receive the file. The method does not
    /// report directory creation, directory listing, snapshot work, or cleanup,
    /// never writes to stderr, never emits terminal-dependent formatting, and
    /// is not idempotent.
    fn report_copy_progress(&self, verbosity: StdoutVerbosity, relpath: String);

    /// Writes one logical displacement progress line when info-level output is
    /// visible.
    ///
    /// At `Info`, `Debug`, and `Trace` verbosity, the emitted line is exactly
    /// `X <relpath>`, with one space after `X` and the slash-separated relative
    /// path supplied by the caller. At `Error` verbosity, no line is emitted.
    /// Files and directories both use this same form, and one logical
    /// displaced path is reported by one caller invocation regardless of how
    /// many peers displace it. The method does not report directory creation,
    /// directory listing, snapshot work, or cleanup, never writes to stderr,
    /// never emits terminal-dependent formatting, and is not idempotent.
    fn report_displacement_progress(&self, verbosity: StdoutVerbosity, relpath: String);

    /// Writes one trace-level copy-slot event.
    ///
    /// At `Trace` verbosity, the emitted line is exactly
    /// `copy-slots active=<n>/<max>` using the caller-supplied global active
    /// file-copy slot count after the event and the caller-supplied global
    /// copy-slot limit. At `Error`, `Info`, and `Debug` verbosity, no line is
    /// emitted. The values describe file-copy slots, not network connection
    /// counts. The method does not enforce concurrency, never writes to stderr,
    /// never emits terminal-dependent formatting, and is not idempotent.
    fn report_copy_slots(&self, verbosity: StdoutVerbosity, active: u32, max: u32);

    /// Writes the final successful sync completion message to stdout.
    ///
    /// The message text is supplied by the caller and is emitted after the sync
    /// operation has successfully completed. It is emitted as a complete plain
    /// line and is visible at every verbosity. The method does not decide
    /// whether the sync succeeded, does not select an exit code, never writes
    /// to stderr, and never emits terminal-dependent formatting. Calls are
    /// emitted in caller order and are not idempotent.
    fn report_completion(&self, verbosity: StdoutVerbosity, message: String);
}
