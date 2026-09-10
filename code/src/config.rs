//! Parsed command line. Produced by `cli::parse`, consumed by the engine.

use crate::output::Verbosity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Canon,
    Normal,
    Subordinate,
}

/// One URL for a peer, after parsing. `normalized` is the identity string
/// per specs/database.md "URL Normalization" (query stripped, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerUrl {
    pub scheme: Scheme,
    /// Display/identity form, e.g. `file:///Users/x/photos` or `sftp://ace@host/path`.
    pub normalized: String,
    /// For file://: absolute local path. For sftp://: absolute remote path.
    pub path: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: String,
    pub port: u16,
    /// Per-URL overrides, seconds.
    pub timeout_conn: Option<u64>,
    pub timeout_idle: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    File,
    Sftp,
}

#[derive(Debug, Clone)]
pub struct PeerSpec {
    pub role: Role,
    /// Primary URL first, then fallbacks in order.
    pub urls: Vec<PeerUrl>,
}

/// What the run does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Sync,
    /// Roll peers back to this timestamp (micros since epoch).
    Rollback(i64),
    /// Roll peers back to just before their newest run.
    Undo,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub peers: Vec<PeerSpec>,
    pub dry_run: bool,
    pub parallel: usize,
    pub retries_copy: u32,
    pub retries_list: u32,
    pub timeout_conn: u64,
    pub timeout_idle: u64,
    pub verbosity: Verbosity,
    /// Patterns from `-x`, in command-line order (applied after built-ins and peer ignore files).
    pub ignore: crate::ignore::IgnoreSet,
    pub keep_bak_days: u64,
    pub keep_del_days: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Sync,
            peers: Vec::new(),
            dry_run: false,
            parallel: 5,
            retries_copy: 3,
            retries_list: 3,
            timeout_conn: 30,
            timeout_idle: 30,
            verbosity: Verbosity::Info,
            ignore: crate::ignore::IgnoreSet::new(),
            keep_bak_days: 90,
            keep_del_days: 180,
        }
    }
}

/// Outcome of parsing the command line.
pub enum Parsed {
    /// No arguments at all: print help, exit 0.
    Help,
    /// Validation error: print the message, then the help text, exit 1.
    Error(String),
    Run(Config),
}
