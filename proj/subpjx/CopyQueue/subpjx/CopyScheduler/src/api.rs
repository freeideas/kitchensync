//! Public interface for the CopyScheduler subproject.
//!
//! CopyScheduler is the run-global execution engine behind the copy queue. It
//! accepts queued file copies, runs them concurrently, and holds the single
//! limit on how many file copies may be active at one instant across the whole
//! run. It also runs the queue's non-copy work items concurrently without
//! letting them consume copy slots. For each queued copy it tracks a per-copy
//! try budget, retries failures by moving the copy to the back of the queue,
//! and reports a final per-copy outcome.
//!
//! CopyScheduler does not perform the transfer itself: when a copy holds a slot
//! it drives SwapTransfer to carry out that one copy through the SWAP staging
//! path. It does not decide which files to copy, does not branch on peer
//! scheme, and does not own the progress-line text; it manages concurrency and
//! retry bookkeeping and surfaces trace events and outcomes to its caller.

use std::sync::Arc;
use std::time::SystemTime;

/// One file copy handed to the scheduler to be scheduled and driven through
/// SwapTransfer.
///
/// The scheduler does not decide these fields and never re-reads them from the
/// source; the sync engine supplies the winning values. Peers are identified by
/// their winning (canonical) URL and paths are relative to the peer root.
pub struct CopyJob {
    /// Source peer, identified by its winning (canonical) URL.
    pub src_peer: String,
    /// Source path relative to the source peer's root.
    pub src_path: String,
    /// Destination peer, identified by its winning (canonical) URL.
    pub dst_peer: String,
    /// Destination path relative to the destination peer's root.
    pub dst_path: String,
    /// The winning modification time to stamp on the destination once the
    /// replacement is in place. The scheduler relays it to SwapTransfer
    /// unchanged.
    pub mod_time: SystemTime,
}

/// Run-global scheduler settings, established once before any copy is enqueued
/// or any non-copy work is submitted.
///
/// A field left `None` selects the documented default. The scheduler's
/// behavior is identical in a dry-run, so no dry-run flag crosses this
/// boundary: whether a single try mutates peer state is SwapTransfer's concern.
pub struct SchedulerConfig {
    /// Single limit on the number of file copies active at one instant across
    /// the whole run, derived from `--max-copies`. `None` selects 10. The
    /// limit is independent of peer scheme, peer count, and connection count,
    /// and only file copies count against it.
    pub max_copies: Option<usize>,
    /// Maximum total number of tries for one queued copy, counting the first
    /// try, derived from `--retries-copy`. `None` selects 3. Try budgets are
    /// tracked per copy and are independent across copies.
    pub retries_copy: Option<u32>,
}

/// A copy-slot trace event the scheduler surfaces to its caller.
///
/// The scheduler owns neither the wording nor the routing of these events; the
/// caller formats the `C`/`X` trace text and sends it to the output component.
pub enum SlotTrace {
    /// A copy acquired a copy slot.
    Acquired,
    /// A copy released the copy slot it held.
    Released,
}

/// The terminal outcome of one enqueued copy.
pub enum CopyOutcome {
    /// The copy succeeded on some try within its budget.
    Succeeded,
    /// The copy exhausted its try budget and is failed for the run.
    Failed,
}

/// Sink the caller (the CopyQueue facade) supplies so the scheduler can surface
/// copy-slot trace events and per-copy outcomes without writing output itself.
///
/// The scheduler keeps stderr empty and never emits these directly; it hands
/// each event to this observer, which routes it to the output component.
pub trait SchedulerObserver: Send + Sync {
    /// Report that `copy` acquired or released a copy slot, so the caller can
    /// route the trace text to the output component.
    fn slot_trace(&self, copy: &CopyJob, trace: SlotTrace);

    /// Report that `copy` reached its terminal outcome. Every enqueued copy
    /// reports exactly one outcome and is never silently dropped.
    fn copy_outcome(&self, copy: &CopyJob, outcome: CopyOutcome);
}

/// The copy-execution scheduler. A single shared instance is used for the whole
/// run, so `Arc<dyn CopyScheduler>` is the handle the CopyQueue facade holds.
pub trait CopyScheduler: Send + Sync {
    /// Establish the run-global copy-slot limit and per-copy try limit, and
    /// register the observer that receives copy-slot trace events and per-copy
    /// outcomes.
    ///
    /// Call this once before any copy is enqueued or any non-copy work is
    /// submitted. A `None` config field takes its documented default
    /// (`max_copies` 10, `retries_copy` 3).
    fn configure(&self, config: SchedulerConfig, observer: Arc<dyn SchedulerObserver>);

    /// Enqueue a file copy and return immediately, without waiting for it to
    /// run.
    ///
    /// A newly enqueued copy is accepted while earlier copies are still
    /// running, so copy work for an already-scanned directory begins while
    /// later directories are still being scanned; the scheduler never waits for
    /// a whole-tree scan before starting copy work. The copy is run under the
    /// run-global copy-slot limit -- at most the configured number of file
    /// copies are active at any instant, independent of peer scheme -- and only
    /// file copies count against that limit.
    ///
    /// While the copy holds a slot the scheduler drives SwapTransfer to perform
    /// one transfer try, then applies the per-copy try budget:
    ///
    /// - When a try fails before the copy reaches its try limit, the copy is
    ///   moved to the back of the queue and other queued work continues.
    /// - When the copy's try count reaches the try limit, it is marked failed
    ///   for the run and is not requeued.
    ///
    /// Try counts are tracked independently per copy; one copy's failed tries
    /// never reduce another copy's budget, and each copy is tried at most its
    /// try-limit times in total. The budget applies identically to local,
    /// SFTP, and mixed-scheme copies. Exactly one outcome (succeeded, or
    /// failed-for-the-run after exhausting tries) is reported through the
    /// observer; the copy is never silently dropped.
    fn enqueue(&self, copy: CopyJob);

    /// Submit non-copy work items to be run concurrently without consuming a
    /// copy slot.
    ///
    /// Every item in the batch is run concurrently with the others and proceeds
    /// even while the copy-slot limit is already full; non-copy work is never
    /// blocked by a full copy limit. The batch is issued together -- for
    /// example, the per-peer directory listings for one directory level run
    /// concurrently rather than one after another. The scheduler treats each
    /// item as opaque work and never branches on what it does.
    fn submit(&self, work: Vec<Box<dyn FnOnce() + Send + 'static>>);

    /// Block until every copy enqueued so far has reached a terminal outcome
    /// and every submitted non-copy work item has finished.
    ///
    /// Called once after the traversal has enqueued all copies and submitted
    /// all work, so the run does not exit while copies or non-copy work are
    /// still running.
    fn wait(&self);
}
