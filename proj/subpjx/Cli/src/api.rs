//! Public specification for the `Cli` subproject.
//!
//! `Cli` turns a raw command-line argument vector into a validated run
//! configuration, or rejects the invocation with an error message and the
//! help text. It only reads the argument strings and the fixed help text:
//! it never connects to a peer, never normalizes a URL into its canonical
//! identity, and never touches a snapshot or a file.

/// The role a peer plays, taken from its leading prefix.
///
/// A leading `+` marks the canon peer, a leading `-` marks a subordinate
/// peer, and no prefix marks a normal bidirectional peer (001.9, 001.10,
/// 001.11). A prefix placed before a bracketed group applies to the whole
/// group (001.15).
pub enum PeerRole {
    /// `+` prefix: the single canon peer. At most one may appear (001.9,
    /// 001.12).
    Canon,
    /// `-` prefix: a subordinate peer. Several may appear in one invocation
    /// (001.10, 001.13).
    Subordinate,
    /// No prefix: a normal bidirectional peer (001.11).
    Normal,
}

/// The per-URL settings recognized as query parameters on a peer URL.
///
/// Only `timeout-conn` and `timeout-idle` are accepted on a peer URL; their
/// values are recorded here (001.16, 001.17). Any other query parameter is a
/// validation failure (001.18, 001.19), so a successfully produced value only
/// ever carries these two. A setting absent from the URL is `None`.
pub struct UrlSettings {
    /// The `timeout-conn` query parameter value, if present (001.16).
    pub timeout_conn: Option<u32>,
    /// The `timeout-idle` query parameter value, if present (001.17).
    pub timeout_idle: Option<u32>,
}

/// A single URL within a peer, with the settings parsed from its query string.
///
/// The URL string is carried through verbatim, without normalizing its
/// identity; canonical normalization is the responsibility of
/// `003_url-normalization` in another component.
pub struct PeerUrl {
    /// The peer URL exactly as it appeared on the command line, un-normalized.
    /// A bare path with no scheme (such as `/path`, `c:\path`, or `./relative`)
    /// is accepted as a local peer, and an `sftp://` URL is accepted as a peer
    /// (001.6, 001.7).
    pub url: String,
    /// The per-URL settings parsed from this URL's query parameters.
    pub settings: UrlSettings,
}

/// One peer: a role plus an ordered list of fallback URLs.
///
/// A single URL yields a peer with one entry in `urls`. Square brackets group
/// several comma-separated URLs into a single peer (a fallback group), in which
/// case `urls` holds them in the order written (001.14). The `role` applies to
/// the whole peer, including every URL in a bracketed group (001.15).
pub struct Peer {
    /// The peer's role, from its leading prefix (001.9, 001.10, 001.11).
    pub role: PeerRole,
    /// The peer's URLs in order. A non-grouped peer has exactly one; a
    /// bracketed group has its members in the written order (001.14).
    pub urls: Vec<PeerUrl>,
}

/// The accepted `--verbosity` levels.
///
/// Only the four words `error`, `info`, `debug`, and `trace` are accepted
/// (001.24); any other value is a validation failure (001.25).
pub enum Verbosity {
    Error,
    Info,
    Debug,
    Trace,
}

/// The global option values placed into the configuration.
///
/// Each integer-valued field carries a positive integer: either the value
/// given on the command line or the option's default. A zero, negative, or
/// non-integer value for any of these is a validation failure (001.22,
/// 001.23), so a successfully produced value always holds a positive integer
/// in each integer field (001.20).
pub struct GlobalOptions {
    /// The `--dry-run` flag (001.20).
    pub dry_run: bool,
    /// The `--max-copies` value, a positive integer (001.20, 001.22).
    pub max_copies: u32,
    /// The `--retries-copy` value, a positive integer (001.20, 001.22).
    pub retries_copy: u32,
    /// The `--retries-list` value, a positive integer (001.20, 001.22).
    pub retries_list: u32,
    /// The `--timeout-conn` value, a positive integer (001.20, 001.22).
    pub timeout_conn: u32,
    /// The `--timeout-idle` value, a positive integer (001.20, 001.22).
    pub timeout_idle: u32,
    /// The `--verbosity` value, one of the four accepted words (001.20,
    /// 001.24).
    pub verbosity: Verbosity,
    /// The `--keep-tmp-days` value, a positive integer (001.20, 001.22).
    pub keep_tmp_days: u32,
    /// The `--keep-bak-days` value, a positive integer (001.20, 001.22).
    pub keep_bak_days: u32,
    /// The `--keep-del-days` value, a positive integer (001.20, 001.22).
    pub keep_del_days: u32,
}

/// A validated run configuration: the result of an accepted invocation.
///
/// Invariants guaranteed by a value of this type: it holds at least two peers,
/// at most one canon (`+`) peer, only recognized global flags, only positive
/// integer values for the integer-valued options, a `verbosity` that is one of
/// the four accepted words, and only well-formed relative exclude paths
/// (001.8, 001.12, 001.20, 001.22, 001.24, 001.28..001.32).
pub struct RunConfig {
    /// The peers, each with its role and its ordered fallback URLs with
    /// per-URL settings. Always at least two (001.8).
    pub peers: Vec<Peer>,
    /// The accepted `-x` exclude paths in the order given. Each is a
    /// well-formed relative path: no leading `/`, no trailing `/`, no `\`
    /// separator, no empty/`.`/`..` segment, and no NUL character
    /// (001.26, 001.27, 001.28, 001.29, 001.30, 001.31, 001.32).
    pub excludes: Vec<String>,
    /// The global option values, or their defaults (001.20).
    pub options: GlobalOptions,
}

/// The outcome of parsing an argument vector.
///
/// Exactly one of three cases. The caller is responsible for the printing and
/// exit code each case prescribes; all of `Cli`'s output is destined for
/// standard output and `Cli` never writes to standard error.
pub enum CliOutcome {
    /// No arguments were given. The caller should print the help text to
    /// standard output, leave standard error empty, and exit 0 (001.1,
    /// 001.2, 002.2, 002.3).
    Help,
    /// The invocation is valid. The caller should run with this configuration.
    Run(RunConfig),
    /// The invocation is invalid. The caller should print this error message
    /// to standard output, then print the verbatim help text after it, then
    /// exit 1 (001.3, 001.4, 001.5, 002.4). The string is the error message
    /// only; it does not include the help text. The first validation error
    /// encountered is sufficient; more than one need not be reported.
    Reject(String),
}

/// Parse and accept/reject a command-line invocation.
///
/// `Cli` is purely a parsing and acceptance/rejection component. It does not
/// resolve, normalize, or connect URLs, does not interpret the meaning of
/// excluded paths, and does not apply option values beyond recording them.
///
/// The `Send + Sync` supertraits let an `Arc<dyn Cli>` be shared as a handle.
pub trait Cli: Send + Sync {
    /// Parse `args` (the argument vector, excluding the program name) into one
    /// of the three [`CliOutcome`] cases.
    ///
    /// With no arguments the result is [`CliOutcome::Help`] (001.1, 001.2). A
    /// valid invocation yields [`CliOutcome::Run`] carrying a [`RunConfig`]
    /// that satisfies every invariant documented on that type. Any validation
    /// failure yields [`CliOutcome::Reject`] carrying the error message
    /// (001.8, 001.12, 001.18, 001.19, 001.21, 001.22, 001.23, 001.25,
    /// 001.28..001.32). The first error encountered is enough to reject.
    fn parse(&self, args: Vec<String>) -> CliOutcome;

    /// Render the help text exactly as written in `specs/help.md`, character
    /// for character, so a caller can print it verbatim to standard output
    /// (002.1). The text is fixed and does not vary with the arguments.
    fn help_text(&self) -> String;
}
