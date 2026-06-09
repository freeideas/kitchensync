//! Public interface for the RunController subproject.
//!
//! RunController drives a single KitchenSync run from end to end once the
//! command line has already been parsed into a validated run configuration. It
//! is the one component that owns the control flow of a run: it sequences and
//! gates the focused services that do the real work and threads the dry-run flag
//! through to them.
//!
//! RunController never parses arguments, never lists or classifies entries
//! itself, never executes a file copy itself, and never reads or writes a
//! snapshot row itself. It connects the peers, decides whether the run may
//! proceed, drives the traversal and copy phases, writes updated snapshots back,
//! disconnects, reports completion, and owns the process exit code. The
//! validated configuration it consumes is produced by the `cli` subproject.

/// The result of a completed call to [`RunController::run`].
///
/// It carries the process exit code the run resolved to and, on a gated error
/// exit, the diagnostic message that must be shown. RunController chooses the
/// exit code and the message; emitting the message is the Output service's job.
pub struct RunOutcome {
    /// The process exit code. A run that passes the gates and completes connect,
    /// traversal, copy, writeback, and disconnect resolves to `0` (006.11). Each
    /// lifecycle gate that fails resolves to `1` (006.2, 006.3, 006.5, 006.7).
    pub exit_code: i32,
    /// The diagnostic message to show on a gated error exit, or `None` on a
    /// successful (exit `0`) run.
    ///
    /// It is `Some` carrying the exact required text for the two
    /// condition-specific gates: `First sync? Mark the authoritative peer with a
    /// leading +` when no reachable peer has snapshot data and no canon peer is
    /// designated (006.4), and `No contributing peer reachable - cannot make
    /// sync decisions` when, after auto-subordination of snapshotless peers, no
    /// contributing peer is reachable (006.6).
    pub message: Option<String>,
}

/// The run orchestrator. A single shared instance drives one whole run, so
/// `Arc<dyn RunController>` is the handle a caller holds.
pub trait RunController: Send + Sync {
    /// Run the whole synchronization for an already-validated configuration and
    /// return its [`RunOutcome`] (the process exit code, and on a gated error
    /// exit the diagnostic message to show).
    ///
    /// `config` is the validated run configuration produced by the `cli`
    /// subproject: the peers with their roles and ordered fallback URLs with
    /// per-URL settings, the command-line excludes, and the global option values
    /// including `--dry-run`. RunController does not parse it further; it
    /// sequences the services that act on it.
    ///
    /// The work happens strictly in this order, and each gate that fails ends
    /// the run before any traversal, snapshot download, or copy is started, so a
    /// run that cannot make valid decisions never mutates a peer:
    ///
    /// Connect and gather the reachable set:
    /// - Connection attempts to all peers are started concurrently rather than
    ///   strictly one peer after another (006.1); the per-peer URL/fallback work
    ///   is delegated to the Transport service.
    /// - A peer whose every URL failed is treated as unreachable and is excluded
    ///   entirely from all later listings and sync decisions (006.12), and its
    ///   snapshot rows are left untouched for the whole run (006.13).
    ///
    /// Gates (each failure ends the run with `exit_code` `1`):
    /// - Fewer than two peers reachable: exit `1` (006.2).
    /// - The designated canon (`+`) peer is unreachable: exit `1` (006.3).
    /// - No reachable peer has snapshot data and no canon peer is designated:
    ///   `message` is the 006.4 text and exit is `1` (006.4, 006.5).
    /// - After auto-subordination of snapshotless peers, no contributing
    ///   (non-subordinate) peer is reachable: `message` is the 006.6 text and
    ///   exit is `1` (006.6, 006.7).
    ///
    /// Recover and prepare snapshots: before the walk, the Snapshot service
    /// recovers any interrupted SWAP and downloads each reachable peer's
    /// snapshot database. This is ordered ahead of traversal and behind a
    /// successful set of gates.
    ///
    /// Traversal and copy (interleaved): the combined-tree walk is driven through
    /// the SyncEngine, and copy work for an already-scanned directory begins
    /// while traversal continues into later directories, with no phase that scans
    /// the whole tree before any copy starts (006.8). The run does not exit until
    /// every enqueued copy has finished (006.9).
    ///
    /// Finish: in a normal run the updated snapshots are written back to their
    /// peers before exit (006.10); then all peers are disconnected and completion
    /// is reported through the Output service. A run that completes all phases
    /// resolves to exit `0` (006.11); any required phase left uncompleted is not
    /// reported as a successful completion.
    ///
    /// Dry-run: when the configuration's `--dry-run` flag is set, the run is read
    /// like a normal run but every peer-mutating step is suppressed: no snapshot
    /// writeback, no copies, and no displacements are applied. RunController
    /// carries the flag into each service it calls rather than re-deciding
    /// mutation at every call site, so the same orchestration sequence serves
    /// both normal and dry-run executions.
    fn run(&self, config: cli::RunConfig) -> RunOutcome;
}
