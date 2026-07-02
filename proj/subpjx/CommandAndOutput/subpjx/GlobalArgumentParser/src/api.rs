#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalArgumentParseResult {
    Help(GlobalCommandOutput),
    ValidationFailure(GlobalCommandOutput),
    Run(GlobalArgumentRunRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalCommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalArgumentRunRequest {
    pub settings: GlobalRunSettings,
    pub peer_operands: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalRunSettings {
    pub dry_run: bool,
    pub max_copies: u32,
    pub retries_copy: u32,
    pub retries_list: u32,
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
    pub verbosity: GlobalVerbosity,
    pub keep_tmp_days: u32,
    pub keep_bak_days: u32,
    pub keep_del_days: u32,
    pub excludes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalVerbosity {
    Error,
    Info,
    Debug,
    Trace,
}

pub trait GlobalArgumentParser: Send + Sync {
    /// Parses the process argument list after the executable name into either
    /// an immediate command output or the global run settings plus the remaining
    /// peer operand strings.
    ///
    /// An empty argument list always returns `GlobalArgumentParseResult::Help`
    /// with stdout equal to the supplied help text verbatim, exit code 0, empty
    /// stderr, and no run request. The parser does not read the help file; the
    /// supplied help text is returned verbatim for help and validation failures.
    ///
    /// Non-help parsing consumes only documented global options before the first
    /// peer operand: `--dry-run`; `--max-copies N`; `--retries-copy N`;
    /// `--retries-list N`; `--timeout-conn N`; `--timeout-idle N`;
    /// `--keep-tmp-days N`; `--keep-bak-days N`; `--keep-del-days N`;
    /// `--verbosity LEVEL`; and repeated `-x RELPATH`. Once peer operands
    /// begin, every remaining argument is preserved as a peer operand string in
    /// its original order, even if it looks like a global option. Repeated `-x`
    /// options append valid relative slash paths to the excludes list in
    /// command-line order.
    ///
    /// Successful parsing applies these defaults: dry run disabled, maximum
    /// copies 10, copy retries 3, listing retries 3, connection timeout 30
    /// seconds, idle timeout 30 seconds, verbosity `info`, TMP retention 2 days,
    /// BAK retention 90 days, deletion-record retention 180 days, and no
    /// excludes. Successful parsing stores numeric settings as positive
    /// integers, stores verbosity as one of error, info, debug, or trace, returns
    /// no stdout, returns empty stderr, does not print, and does not terminate
    /// the process.
    ///
    /// A non-help validation failure owned by this parser returns
    /// `GlobalArgumentParseResult::ValidationFailure` with stdout containing
    /// one plain text error message followed by the supplied help text, exit
    /// code 1, empty stderr, and no run request. Validation failures include an
    /// unrecognized flag in the global option area; a valued global option with
    /// no following value; a numeric option value that is zero, negative, empty,
    /// fractional, or non-numeric; an unsupported verbosity value; `-x` with no
    /// value; or an `-x` value that is not a valid relative slash path. The
    /// parser does not validate peer counts, peer roles, peer URL forms,
    /// fallback groups, URL query settings, or any sync-time behavior.
    fn parse_global_arguments(
        &self,
        args: Vec<String>,
        help_text: String,
    ) -> GlobalArgumentParseResult;
}
