//! Public interface for the `Clock` subproject.
//!
//! Clock owns the single timestamp string format used everywhere a timestamp
//! appears in a run, and the one run-wide generator that hands out fresh "now"
//! values. It does no I/O and reaches no filesystem or database; its only state
//! is the highest value it has handed out so far, which it uses to keep every
//! fresh value strictly greater than the last.
//!
//! The fixed format is `YYYY-MM-DD_HH-mm-ss_ffffffZ`: UTC, microsecond
//! precision (six fractional-second digits), trailing `Z`. Because the fields
//! run from most significant to least significant and each is zero-padded to a
//! fixed width, sorting these values as plain strings orders them
//! chronologically, and the value is safe to embed in a filesystem path.

/// The run-wide source of fresh, strictly-increasing timestamp strings.
///
/// The `Send + Sync` supertraits let the single instance be shared as
/// `Arc<dyn Clock>` across the whole run. The strictly-increasing state is
/// process-wide: one source serves the entire run, so two callers asking at the
/// same microsecond still receive distinct, ordered values.
pub trait Clock: Send + Sync {
    /// Produce a fresh "now" timestamp.
    ///
    /// Reads the current UTC time and formats it as
    /// `YYYY-MM-DD_HH-mm-ss_ffffffZ` (015.1, 015.2, 015.3). The returned value
    /// is strictly greater than every value this source has returned before in
    /// this process: when the formatted current time is not greater than the
    /// last value handed out, the source advances by one microsecond and
    /// re-formats, repeating until the result is strictly greater (015.8). No
    /// two fresh values are ever equal, and the values sort chronologically as
    /// plain strings (015.4).
    ///
    /// Callers use this value for each `last_seen` write (015.6) and for each
    /// BAK/ or TMP/ directory name created during the run (015.7); the same
    /// format also governs database timestamp columns and log output (015.5).
    ///
    /// This is the only timestamp-generating operation. `deleted_time` values
    /// are never produced here: they are copied from a row's existing
    /// `last_seen` (015.9, 015.10) and are exempt from the strictly-increasing
    /// rule, so the generator is asked only for the `last_seen` and
    /// directory-name sites.
    ///
    /// The only failure surface is formatting the current time, which is not
    /// expected to fail under normal operation; the method therefore returns
    /// the value directly rather than a `Result`.
    fn now(&self) -> String;
}
