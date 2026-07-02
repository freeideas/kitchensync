use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandParseResult {
    Help(CommandProcessOutput),
    ValidationFailure(CommandProcessOutput),
    Run(RunRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRequest {
    pub settings: RunSettings,
    pub peers: Vec<Peer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSettings {
    pub dry_run: bool,
    pub max_copies: u32,
    pub retries_copy: u32,
    pub retries_list: u32,
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
    pub verbosity: Verbosity,
    pub keep_tmp_days: u32,
    pub keep_bak_days: u32,
    pub keep_del_days: u32,
    pub excludes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verbosity {
    Error,
    Info,
    Debug,
    Trace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub role: PeerRole,
    pub fallback_targets: Vec<PeerTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRole {
    Canon,
    Subordinate,
    Normal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerTarget {
    pub location: PeerLocation,
    pub connection: UrlConnectionSettings,
    pub normalized_identity: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerLocation {
    Local(LocalPeerTarget),
    Sftp(SftpPeerTarget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPeerTarget {
    pub path_or_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpPeerTarget {
    pub host: String,
    pub username: String,
    pub username_was_explicit: bool,
    pub password: Option<String>,
    pub port: u16,
    pub absolute_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UrlConnectionSettings {
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerIdentityError {
    pub target: PeerLocation,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputEvent {
    ArgumentValidationFailure(ArgumentValidationFailureOutput),
    FirstSyncNeedsCanon,
    NoContributingPeerReachable,
    ErrorDiagnostic(SyncErrorDiagnostic),
    FailedFileTransfer(FailedFileTransferDiagnostic),
    CopyProgress { relpath: String },
    DisplacementProgress { relpath: String },
    CopySlots { active: u32, max: u32 },
    Completion { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgumentValidationFailureOutput {
    pub error_message: String,
    pub help_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncErrorDiagnostic {
    pub kind: SyncErrorKind,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncErrorKind {
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
pub struct FailedFileTransferDiagnostic {
    pub relpath: String,
    pub destination_peer_url: String,
    pub phase: FileTransferPhase,
    pub transport_error: Option<TransportErrorCategory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTransferPhase {
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

pub trait CommandAndOutput: Send + Sync {
    /// Parses product arguments after the executable name into either a
    /// complete run request or an immediate command outcome.
    ///
    /// An empty argument vector is always the help invocation and returns the
    /// exact fenced help screen on stdout, exit code 0, empty stderr, and no run
    /// request. A non-help validation failure returns one plain error message
    /// followed by that same help text on stdout, exit code 1, empty stderr, and
    /// no run request. Repeated calls with the same inputs return the same
    /// outcome and do not emit output. Successful parsing preserves peer
    /// operand order, fallback URL order, and repeated exclude order; accepts
    /// at least two peers and at most one canon peer; records URL-level timeout
    /// overrides; applies the documented defaults; stores only positive integer
    /// settings; and stores normalized peer identities suitable for later
    /// comparison and lookup. It never connects to peers, creates directories,
    /// authenticates, lists files, makes sync decisions, writes stdout, or
    /// writes stderr.
    fn parse_command(
        &self,
        args: Vec<String>,
        current_working_directory: PathBuf,
        current_os_username: String,
    ) -> CommandParseResult;

    /// Normalizes one already-accepted peer target into the only URL identity
    /// form callers may use for equality, duplicate detection, snapshot lookup,
    /// or any other peer identity lookup.
    ///
    /// Local paths and Windows drive paths become absolute file URL identities,
    /// with relative paths resolved from the supplied current working
    /// directory. The normalized identity lowercases schemes and hostnames,
    /// removes SFTP port 22 while preserving non-default ports, collapses
    /// consecutive path slashes, removes trailing path slashes, decodes
    /// percent-encoded unreserved path characters, leaves percent-encoded
    /// reserved path characters encoded, strips query strings, inserts the
    /// supplied operating-system username for SFTP targets with no explicit
    /// username, and preserves explicit SFTP usernames. The operation reports
    /// only failures that prevent forming an identity from an already-accepted
    /// target. Repeated calls with the same inputs return the same identity or
    /// error. The operation never writes stdout or stderr.
    fn normalize_peer_identity(
        &self,
        target: PeerLocation,
        current_working_directory: PathBuf,
        current_os_username: String,
    ) -> Result<String, PeerIdentityError>;

    /// Writes one already-decided output event to stdout using the supplied
    /// verbosity and never writes to stderr.
    ///
    /// Output is line based, plain text, terminal-independent, and emitted in
    /// the same order callers invoke this method. Error diagnostics are visible
    /// at every verbosity. Info-level copy and displacement progress lines are
    /// suppressed only at `Verbosity::Error`; debug has no additional messages
    /// and is observably the same as info; trace includes copy-slot events as
    /// exactly `copy-slots active=<n>/<max>`. Copy progress emits exactly one
    /// `C <relpath>` line per logical copied path, displacement progress emits
    /// exactly one `X <relpath>` line per logical displaced path, and no
    /// progress line is emitted for directory creation, directory listing,
    /// snapshot work, or BAK, TMP, or SWAP cleanup. Argument validation failure
    /// output is one error message followed by the exact help text. The first
    /// sync event writes exactly
    /// `First sync? Mark the authoritative peer with a leading +`. The
    /// no-contributing-peer event writes exactly
    /// `No contributing peer reachable - cannot make sync decisions`. Failed
    /// file-transfer diagnostics include the relative path, destination peer
    /// URL, one of the specified phase labels, and the transport error category
    /// when present. This operation is not idempotent: repeated calls write
    /// repeated output.
    fn write_output(&self, verbosity: Verbosity, event: OutputEvent);
}
