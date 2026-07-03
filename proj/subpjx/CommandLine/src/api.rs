#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandLineParseResult {
    Help,
    ValidationError(CommandLineValidationError),
    Run(CommandLineRunRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLineValidationError {
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLineRunRequest {
    pub settings: CommandLineSettings,
    pub peers: Vec<CommandLinePeer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLineSettings {
    pub dry_run: bool,
    pub max_copies: u64,
    pub retries_copy: u64,
    pub retries_list: u64,
    pub timeout_conn_seconds: u64,
    pub timeout_idle_seconds: u64,
    pub verbosity: CommandLineVerbosity,
    pub excludes: Vec<String>,
    pub keep_tmp_days: u64,
    pub keep_bak_days: u64,
    pub keep_del_days: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandLineVerbosity {
    Error,
    Info,
    Debug,
    Trace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLinePeer {
    pub role: CommandLinePeerRole,
    pub urls: Vec<CommandLineUrlAlternative>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandLinePeerRole {
    Normal,
    Canon,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLineUrlAlternative {
    pub url: String,
    pub timeout_conn_seconds: Option<u64>,
    pub timeout_idle_seconds: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLineProcessOutput {
    pub stdout: String,
    pub exit_code: i32,
}

pub trait CommandLine: Send + Sync {
    /// Parses process arguments after the executable name has been removed.
    /// The result is always exactly one of help, one validation error, or one
    /// complete run request. With no arguments this returns help. Non-help
    /// invocations require at least two peers, accept at most one canon peer,
    /// accept multiple subordinate peers, reject unrecognized flags, and
    /// reject invalid option values. A complete run request keeps peers in
    /// user-supplied order and keeps each peer's fallback URL alternatives in
    /// user-supplied order, with any leading `+` or `-` applying to the whole
    /// bracketed group. Bare local paths are represented for downstream work as
    /// file URLs without applying persistent identity normalization. SFTP URLs
    /// use absolute remote paths and accept percent-encoded `@` and `:`
    /// characters in passwords. Complete run requests contain only positive
    /// numeric option values, default omitted global options to dry-run off,
    /// max-copies 10, retries-copy 3, retries-list 3, timeout-conn 30 seconds,
    /// timeout-idle 30 seconds, verbosity info, keep-tmp-days 2,
    /// keep-bak-days 90, and keep-del-days 180, contain only valid relative
    /// slash-path excludes, and contain only `timeout-conn` and `timeout-idle`
    /// per-URL query settings.
    fn parse(&self, args: Vec<String>) -> CommandLineParseResult;

    /// Builds the no-argument help output. The stdout text is exactly the
    /// help screen from the product help specification, the exit code is 0,
    /// and no stderr output is part of this result.
    fn help_output(&self) -> CommandLineProcessOutput;

    /// Builds validation-failure output for one parse error. The stdout text
    /// contains one error message followed by the same help screen returned by
    /// `help_output`, the exit code is 1, and no stderr output is part of this
    /// result. Calling this repeatedly with the same error is idempotent.
    fn validation_error_output(
        &self,
        error: &CommandLineValidationError,
    ) -> CommandLineProcessOutput;

    /// Builds the successful completion output owned by the command-line
    /// surface. The stdout text is exactly one `sync complete` line for every
    /// verbosity level, the exit code is 0, and no stderr output is part of
    /// this result. Calling this repeatedly is idempotent.
    fn sync_complete_output(&self) -> CommandLineProcessOutput;

    /// Returns whether a diagnostic at `message_verbosity` is emitted when the
    /// run is configured with `configured_verbosity`. Error messages are
    /// emitted at every setting, each higher verbosity includes all lower
    /// levels, and debug currently has the same observable output as info.
    fn should_emit(
        &self,
        configured_verbosity: CommandLineVerbosity,
        message_verbosity: CommandLineVerbosity,
    ) -> bool;
}
