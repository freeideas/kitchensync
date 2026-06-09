//! Public interface for the CopyQueue subproject.
//!
//! CopyQueue executes the file copies the sync engine decides to perform. It
//! runs those copies concurrently under one global copy-slot limit shared
//! across the whole run, retries failed copies up to a per-copy try limit, and
//! makes each replacement recoverable through SWAP staging. It also owns the
//! TMP/SWAP/BAK staging areas under each directory's `.kitchensync/`.
//!
//! CopyQueue never decides which files to copy or which modification time wins;
//! those decisions arrive from the caller as enqueued copy requests. CopyQueue
//! only carries them out safely and reports progress through the output service.

use std::time::{Duration, SystemTime};

/// A single file copy to perform, as decided by the sync engine.
///
/// Peers are identified by their winning (canonical) URL; paths are relative to
/// the peer root and slash-separated.
pub struct CopyRequest {
    /// Source peer, identified by its winning (canonical) URL.
    pub src_peer: String,
    /// Source path relative to the source peer's root.
    pub src_path: String,
    /// Destination peer, identified by its winning (canonical) URL.
    pub dst_peer: String,
    /// Destination path relative to the destination peer's root.
    pub dst_path: String,
    /// The winning modification time to stamp on the destination once the
    /// replacement is in place. It is set verbatim and is never re-read from
    /// the source file.
    pub mod_time: SystemTime,
    /// Called exactly once when the copy succeeds, before the copy slot is
    /// released. `None` means no action on success.
    pub on_success: Option<Box<dyn FnOnce() + Send>>,
}

/// Run-global settings for the copy queue.
///
/// A field left `None` selects the documented default. These settings are
/// established once per run, before any copy is enqueued or any SWAP recovery
/// or cleanup is requested.
pub struct CopyConfig {
    /// Single global limit on the number of file copies that may hold a slot at
    /// one instant across the whole run. `None` selects 10. The limit is
    /// independent of peer scheme, peer count, and connection count, and only
    /// file copies count against it.
    pub copy_slot_limit: Option<usize>,
    /// Maximum total number of tries for one queued copy, counting the first
    /// try. `None` selects 3. Try budgets are tracked per copy and are
    /// independent across copies.
    pub copy_try_limit: Option<u32>,
    /// Age beyond which a `.kitchensync/BAK/<timestamp>/` entry is purged,
    /// judged from its `<timestamp>` directory-name component. `None` selects
    /// 90 days.
    pub bak_retention: Option<Duration>,
    /// Age beyond which a `.kitchensync/TMP/<timestamp>/` entry is purged,
    /// judged from its `<timestamp>` directory-name component. `None` selects
    /// 2 days.
    pub tmp_retention: Option<Duration>,
    /// When true, every peer-mutating operation is suppressed: no copy is
    /// performed, SWAP recovery during traversal is skipped, and BAK/TMP
    /// cleanup is skipped.
    pub dry_run: bool,
}

/// The copy-execution service. A single shared instance is used for the whole
/// run, so `Arc<dyn CopyQueue>` is the handle other components hold.
pub trait CopyQueue: Send + Sync {
    /// Establish the run-global settings (copy-slot limit, per-copy try limit,
    /// BAK/TMP retention, and dry-run mode) for the run.
    ///
    /// Call this once before any other operation. A `None` field takes its
    /// documented default.
    fn configure(&self, config: CopyConfig);

    /// Enqueue a file copy and return immediately, without waiting for it to
    /// run.
    ///
    /// A newly enqueued copy is accepted while earlier copies are still
    /// running, so copy work for an already-scanned directory begins while
    /// later directories are still being scanned; the queue never waits for a
    /// whole-tree scan before starting. The copy is scheduled under the global
    /// copy-slot limit, and only file copies count against that limit.
    ///
    /// Each copy that would replace an existing destination follows this
    /// ordered, recoverable SWAP sequence, never writing the destination in
    /// place:
    ///
    /// 1. Recover or fail any existing SWAP directory for the target basename.
    /// 2. Write the new content to
    ///    `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new`.
    /// 3. If a file exists at the target, move it to that SWAP `old`.
    /// 4. Rename the SWAP `new` file onto the final target path.
    /// 5. Set the destination's modification time to `request.mod_time`.
    /// 6. If SWAP `old` exists, archive it to BAK.
    /// 7. Remove the empty SWAP directories.
    ///
    /// The `<encoded-basename>` segment is the target basename percent-encoded
    /// to a single path segment.
    ///
    /// Retry: when a try fails before the copy reaches its try limit, the copy
    /// is moved to the back of the queue and other work continues; when its try
    /// count reaches the limit it is marked failed for the run and is not
    /// requeued. The copy is tried at most its try-limit times in total.
    ///
    /// Error obligations during a copy:
    /// - Transfer failure before SWAP `old` exists deletes the staged SWAP
    ///   `new`, then requeues or fails the copy by its try count.
    /// - Failure to move the existing destination to SWAP `old` leaves the
    ///   original destination in place and skips this copy for the run.
    /// - Transfer failure after SWAP `old` exists leaves the SWAP state in
    ///   place for later recovery.
    /// - Failure to archive SWAP `old` to BAK after the replacement is in place
    ///   leaves SWAP `old` in place for later recovery.
    ///
    /// The per-copy outcome (succeeded, or failed-for-the-run after exhausting
    /// tries) is reported through the output service, not returned here. In a
    /// dry-run the copy is not performed.
    fn enqueue(&self, request: CopyRequest);

    /// Block until every copy enqueued so far has reached a terminal outcome
    /// (succeeded, or failed-for-the-run after exhausting tries).
    ///
    /// Called once after the traversal has enqueued all copies, so the run does
    /// not exit while copies are still running.
    fn wait(&self);

    /// Run the given jobs concurrently on the shared executor and block until
    /// all of them have finished.
    ///
    /// This is the shared concurrent executor the caller uses to issue the
    /// directory listings for all reachable peers at a given directory level at
    /// the same time rather than one after another. Each job performs one peer's
    /// work (typically a transport listing) and stores its own result in state
    /// the closure captured; the method returns only once every job has
    /// completed.
    ///
    /// The jobs run on the same executor as file copies but never consume a copy
    /// slot, so they proceed at full concurrency even while the copy-slot limit
    /// is full. This operation only schedules the work; it reads and mutates no
    /// peer state of its own and so behaves identically in a normal run and a
    /// dry-run.
    fn run_in_parallel<'scope>(&self, jobs: Vec<Box<dyn FnOnce() + Send + 'scope>>);

    /// Recover the SWAP state for a directory on a peer, before that directory's
    /// live entries are listed for sync decisions.
    ///
    /// Each `.kitchensync/SWAP/<encoded-basename>` directory is recovered
    /// according to the presence of `old`, `new`, and the target:
    /// - `old` present, target present: move `old` to BAK, remove the SWAP dir.
    /// - `old` present, `new` present, target missing: rename `new` to the
    ///   target, move `old` to BAK, remove the SWAP dir.
    /// - `old` present, `new` missing, target missing: rename `old` back to the
    ///   target, remove the SWAP dir.
    /// - `old` missing, `new` present, target present: delete `new`, remove the
    ///   SWAP dir.
    /// - `old` missing, `new` present, target missing: rename `new` to the
    ///   target, remove the SWAP dir.
    ///
    /// Returns `true` when recovery succeeded. On `false`, the caller treats
    /// this peer's listing for the directory as failed and excludes the peer
    /// from that directory subtree. In a dry-run this peer-mutating recovery is
    /// skipped and success is reported.
    fn recover_swap(&self, peer: &str, dir_path: &str) -> bool;

    /// Run aged BAK/TMP cleanup for a directory on a peer, after the union of
    /// entry names at that directory level has been processed.
    ///
    /// Inspects the peer's `.kitchensync/` directly even though the built-in
    /// exclude removes it from synced listings. Removes each
    /// `.kitchensync/BAK/<timestamp>/` entry older than the BAK retention limit
    /// and each `.kitchensync/TMP/<timestamp>/` entry older than the TMP
    /// retention limit, judging age from the `<timestamp>` name. Entries not
    /// older than their limit are left in place, and SWAP is never purged by
    /// age.
    ///
    /// Cleanup is best-effort; any failure is reported through the output
    /// service rather than returned. In a dry-run cleanup is skipped.
    fn cleanup(&self, peer: &str, dir_path: &str);
}
