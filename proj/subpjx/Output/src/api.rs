//! Public interface for the `Output` subproject.
//!
//! Output is the single channel through which KitchenSync emits everything a
//! user sees. Callers hand it progress lines and diagnostics; Output decides
//! whether to print each one based on the configured verbosity level, writes
//! all of it to standard output, and never writes to standard error.

/// The run's verbosity level, ordered least-to-most verbose as
/// `Error` < `Info` < `Debug` < `Trace`.
///
/// The levels are cumulative: each level emits everything the lower levels emit
/// plus its own additions (023.10). `Debug` is observationally identical to
/// `Info`; no message is defined that `Debug` emits but `Info` does not (023.13).
pub enum Verbosity {
    /// Error and nonfatal diagnostics only (023.11).
    Error,
    /// Progress lines and everything `Error` emits (023.12).
    Info,
    /// Identical to `Info`; defined for completeness (023.13).
    Debug,
    /// Trace events and everything `Info` emits (023.14).
    Trace,
}

/// The phase of a file transfer that failed, as carried by a failed-transfer
/// diagnostic. The reported phase is restricted to exactly these values
/// (023.17).
pub enum FailedPhase {
    ReadSource,
    WriteSwapNew,
    MoveExistingToSwapOld,
    RenameFinal,
    SetModTime,
    ArchiveOld,
    Cleanup,
}

/// The single channel for user-visible output.
///
/// `Send + Sync` so `Arc<dyn Output>` is a shareable handle across the
/// concurrent components that report through it.
///
/// Emitting is fire-and-forget from the caller's view: no operation returns an
/// error and no caller has to handle Output failing. Output's obligation is to
/// honor the verbosity threshold and the line format for every message it is
/// given (023.1, 023.2). Output reports only what callers hand it; it does not
/// decide which paths were copied or displaced and does not own the meaning of
/// any error condition.
pub trait Output: Send + Sync {
    /// Set the run's verbosity level. A message is emitted only when this level
    /// is at or above the level the message belongs to; messages below the
    /// threshold are silently dropped.
    fn set_verbosity(&self, level: Verbosity);

    /// Emit a copy progress line for a path copied to one or more peers.
    ///
    /// Emitted at `Info` or higher (023.12). Emits exactly one line regardless
    /// of how many peers received the path (023.7). The line is the letter `C`,
    /// a single space, then `relpath` -- the slash-separated relative path from
    /// the sync root (023.6). Lines appear in the order the actions happen, one
    /// plain line per action (023.3), and are identical whether or not stdout
    /// is a terminal (023.4).
    fn copied(&self, relpath: &str);

    /// Emit a displace/delete progress line for a path displaced or deleted on
    /// one or more peers.
    ///
    /// Emitted at `Info` or higher (023.12). Emits exactly one line for both
    /// files and directories, regardless of how many peers (023.8). The line is
    /// the letter `X`, a single space, then `relpath` -- the slash-separated
    /// relative path from the sync root (023.6), ordered with the other
    /// progress lines (023.3) and identical on or off a terminal (023.4).
    fn displaced(&self, relpath: &str);

    /// Emit an error or nonfatal diagnostic.
    ///
    /// Emitted at `Error` or higher (023.11). Output owns only the format and
    /// the verbosity gating of the diagnostic; the conditions that trigger it
    /// live with their own behaviors.
    fn diagnostic(&self, message: &str);

    /// Emit a failed-transfer diagnostic for a file transfer that failed.
    ///
    /// Emitted at `Error` or higher (023.11). The diagnostic identifies the
    /// relative path, the destination peer URL, the failed phase, and the
    /// transport error category when one is available (023.16); pass `None` for
    /// `error_category` when no category is available.
    fn transfer_failed(
        &self,
        relpath: &str,
        peer_url: &str,
        phase: FailedPhase,
        error_category: Option<&str>,
    );

    /// Emit a copy-slot trace event.
    ///
    /// Emitted only at `Trace` (023.14). Each copy-slot acquire and release
    /// event is the line `copy-slots active=<active>/<max>` (023.15).
    fn copy_slots(&self, active: usize, max: usize);
}
