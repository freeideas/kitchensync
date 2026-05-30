# sync API

Rust module path: `kitchensync::sync`.

The `sync` module exports the combined-tree traversal and reconciliation API
used by the root run orchestration. It owns traversal order, exclude handling,
snapshot-backed classification, group-outcome decisions, snapshot update
timing, inline operation dispatch, copy-task submission, and final copy result
consumption for one prepared KitchenSync run.

The module does not export traversal cursors, vote records, candidate sets,
path-group internals, decision-rule helpers, or child implementation modules.
Other modules may rely only on the public contract documented here.

## Imported Contracts

The public API names these root-owned or sibling-owned contracts without
redefining them:

- `RunConfig`: dry-run flag, listing retry count, copy retry count, excludes,
  copy limits, retention settings, timeouts, and verbosity.
- `PeerSession`, `PeerId`, and `EffectivePeerRole`: connected peer handles and
  canon, contributing, or subordinate role information.
- `RelPath`: validated slash-separated path relative to the sync root. The
  root directory is represented by the root path value and is rendered as `.` in
  progress events.
- `EntryMeta`, `EntryKind`, and `Timestamp`: live filesystem metadata and
  snapshot timestamp values.
- `SnapshotStore`, `SnapshotRow`, and `SnapshotEntryKind`: per-peer snapshot
  lookup and mutation API.
- `OperationExecutor`, `OperationError`, and operation report types: inline
  recovery, displacement, directory creation, retention cleanup, and copy
  attempt behavior.
- `CopyScheduler`, `CopyTask`, `CopyResult`, and `SchedulerSummary`: queued
  file-copy scheduling and terminal copy accounting.
- `DiagnosticSink`, `ProgressSink`, `DiagnosticEvent`, and `ProgressEvent`:
  structured stdout-renderable diagnostics and progress output.
- `TransportError`: normalized transport error category used for listing and
  operation failures.

`sync` must not publicly expose alternate representations for these concepts.
If a contract becomes necessary to more than one first-layer module, it belongs
at the nearest shared ancestor rather than inside a private `sync` child
module.

## Public Function

```rust
pub fn run(run: SyncRun<'_>) -> SyncReport;
```

Runs one sync traversal over the connected peer set supplied by startup. The
function starts at the sync root, walks the combined live tree in pre-order,
submits copy work as soon as eligible paths are found, closes the copy input
after traversal, waits for the scheduler to finish accepted copy work, applies
successful-copy snapshot completion updates, and returns an owned report.

The function is synchronous from the caller's perspective: it returns only
after traversal has ended and all accepted queued copy work has reached a
terminal state. Internal listing and copy concurrency are implementation
details or are owned by the supplied scheduler and operation contracts.

Ordinary peer, listing, operation, and copy failures are reported through the
diagnostic sink and summarized in `SyncReport`; they are not panics. Startup
validation such as too few reachable peers, first-sync canon requirements, and
snapshot preparation failures occurs before this function is called.

## Public Input Types

```rust
pub struct SyncRun<'a> {
    pub config: &'a RunConfig,
    pub peers: &'a mut [SyncPeer<'a>],
    pub operations: &'a dyn OperationExecutor,
    pub copy_scheduler: &'a CopyScheduler,
    pub diagnostics: &'a dyn DiagnosticSink,
    pub progress: &'a dyn ProgressSink,
}
```

`SyncRun` contains all run-scoped dependencies needed by `sync`. The peer set
is exactly the reachable peer set for this run; unreachable peers must already
have been removed by startup and are not represented here.

```rust
pub struct SyncPeer<'a> {
    pub session: &'a PeerSession,
    pub snapshot: &'a mut SnapshotStore,
}
```

`SyncPeer` pairs one connected peer session with that peer's mutable local
snapshot store. The `PeerId` in `session` and `snapshot` must identify the same
peer. A peer's effective role is read from `session.effective_role`.

The slice order must be stable for one run. `sync` preserves transport-supplied
filenames when it asks operations or the copy scheduler to act on a path.

## Public Report Types

```rust
pub struct SyncReport {
    pub completed: bool,
    pub traversal: TraversalReport,
    pub copies: SchedulerSummary,
    pub skipped: Vec<SkippedSubtree>,
    pub failures: Vec<SyncFailure>,
}
```

`completed` is `true` only when traversal finished without unrecovered
decision-blocking failures and the scheduler reported no terminal copy
failures. A report with `completed = false` can still contain successful
operations, snapshot updates, and completed copies from unaffected paths.

```rust
pub struct TraversalReport {
    pub scanned_directories: u64,
    pub decided_entries: u64,
    pub enqueued_copies: u64,
}
```

`TraversalReport` contains stable run accounting for root-level result mapping
and tests. It is not a traversal cursor and cannot be used to resume a run.

```rust
pub struct SkippedSubtree {
    pub directory: RelPath,
    pub reason: SkippedSubtreeReason,
}
```

`SkippedSubtree` records a directory subtree where `sync` intentionally did not
make decisions.

```rust
pub enum SkippedSubtreeReason {
    CanonListingUnavailable { peer_id: PeerId },
    NoContributingPeerListed,
}
```

`CanonListingUnavailable` means the canon peer exhausted listing or pre-listing
SWAP recovery attempts for that directory, so no peer may supply decisions in
the subtree. `NoContributingPeerListed` means every active contributing peer was
removed by listing failure at that directory, so subordinate-only entries in
that subtree were ignored.

```rust
pub enum SyncFailure {
    Listing {
        peer_id: PeerId,
        directory: RelPath,
        attempts: usize,
        canon: bool,
        error: TransportError,
    },
    SwapRecovery {
        peer_id: PeerId,
        directory: RelPath,
        attempts: usize,
        canon: bool,
        error: OperationError,
    },
    Operation {
        peer_id: PeerId,
        path: RelPath,
        error: OperationError,
    },
    Copy {
        result: CopyResult,
    },
    InvalidRunInput {
        reason: SyncInputError,
    },
}
```

`SyncFailure` values own enough normalized context for root result mapping and
tests. Detailed human-readable output remains owned by the diagnostic sink.

```rust
pub enum SyncInputError {
    EmptyPeerSet,
    MissingSnapshotStore { peer_id: PeerId },
    SnapshotPeerMismatch { peer_id: PeerId },
    NoContributingPeer,
    MoreThanOneCanonPeer,
}
```

`SyncInputError` describes caller contract violations discovered by `sync`.
Well-formed root orchestration should not produce these errors, but the run
report exposes them so the module can fail closed without panicking.

## Traversal Contract

`run` processes directories in recursive pre-order. For each directory it
reports scan progress, optionally performs user-entry SWAP recovery, starts
listing for every peer active at that directory before awaiting any listing
result, retries failed listings up to `RunConfig.retries_list` total attempts,
then applies subtree-scoped failure rules.

A non-canon peer that exhausts listing or pre-listing SWAP recovery attempts is
excluded only from that directory subtree for the current run. A canon peer
failure skips all decisions, operations, copies, cleanup-driven snapshot
changes, recursion, and snapshot row changes for every peer under that
directory. If all active contributing peers fail at a directory, the subtree is
skipped and subordinate-only entries are not processed there.

Candidate names come only from live listings. Listed names from active
contributing peers are always considered. Listed names from active subordinate
peers are considered only while at least one contributing peer remains active
for the directory, so subordinate-only paths can be displaced when the group
outcome is absence.

Built-in and command-line excludes are removed before snapshot lookup,
classification, decisions, operations, copies, recursion, and snapshot updates.
Command-line excludes match exact relative file paths and directory subtree
prefixes. Existing peer contents at excluded paths are left untouched.

Candidate entries are processed in deterministic case-insensitive
lexicographic order, with the original case-sensitive name as the tie-breaker.

## Decision Contract

Subordinate peers never contribute live entries or snapshot history to group
outcomes. They receive creates, copies, and displacements needed to match the
selected outcome.

When a canon peer is active at a directory, the canon peer's live file, live
directory, or absence at a candidate path is authoritative.

Without an active canon peer, file outcomes are selected from contributing live
file candidates by newest modification time using the required five-second
tolerance. Tied candidates with different sizes choose the larger file.
Existing data wins over deletion on ties. A deletion estimate wins only when it
is more than five seconds newer than the newest live file candidate.

Directory modification time is ignored. Any contributing live directory selects
directory existence unless canon behavior or a file-vs-directory conflict rule
overrides it. If no contributing peer has a live directory and snapshot rows
prove absence or tombstones, the outcome is directory absence. If no
contributing peer has a vote or snapshot row for the path, the outcome is
absence.

Non-canon file-vs-directory conflicts resolve to a file outcome, with the
winning file chosen by the normal file rules over contributing live file
entries only.

The rule records used to classify entries and choose outcomes are private. The
public API exposes only the final effects through operations, copy tasks,
snapshot mutations, diagnostics, and the run report.

## Snapshot Contract

`sync` reads and mutates snapshots only through the supplied `SnapshotStore`
values. It must not depend on SQLite tables, path hashes, row identifiers,
local database paths, or snapshot lifecycle internals.

Snapshot rows are updated only for peers and paths whose live state was
observed or whose requested operation or copy reached the required success
point:

- confirmed listed files are upserted with live metadata, fresh `last_seen`,
  and no `deleted_time`;
- intended destination copies are upserted with winning metadata and no
  `deleted_time`, without changing `last_seen` before copy success;
- successful copy results set destination `last_seen` to a fresh timestamp;
- failed copies leave destination `last_seen` unchanged;
- successful directory creation marks the directory present with fresh
  `last_seen`;
- confirmed absence marks existing non-tombstone rows deleted using their
  previous `last_seen` as the deletion estimate;
- successful displacement marks the displaced entry deleted, and displaced
  directories request the snapshot store's same-peer subtree cascade;
- failed displacement and failed directory creation leave affected rows
  unchanged.

`sync` never updates snapshot rows for excluded paths, unreachable peers,
peers excluded from a failed listing subtree, or any peer under a subtree
skipped because canon listing failed.

## Operation And Copy Contract

Deletion and type-conflict displacement are executed inline through
`OperationExecutor`; they are never submitted as file-copy work. Missing
directories are created inline. File-existence outcomes submit `CopyTask`
values only for active peers whose live file is absent or does not match the
winner by byte size and five-second modification-time tolerance.

Copy work is submitted as soon as traversal discovers eligible work. `sync`
waits for the supplied `CopyScheduler` to finish all accepted and retried work
before returning. Copy retry accounting, copy-slot limits, worker execution,
and transfer progress rendering belong to `runtime` and `operations`, not to
`sync`.

## Dry-Run Contract

Dry-run mode still performs traversal, classification, decision-making, local
snapshot updates, copy submission, scheduler execution, source reads, progress
events, and diagnostics. In dry-run mode `sync` skips peer-side user-entry SWAP
recovery before listing and skips peer-side BAK/TMP retention cleanup after
directory processing.

For creates, displacements, and copies, `sync` still calls the operation and
copy-scheduler contracts with the dry-run run configuration. Peer-side mutation
suppression is owned by `operations`.

## Ownership And Visibility Rules

- `SyncRun` borrows all run dependencies for the duration of `run`; `sync` does
  not retain references after `run` returns.
- `sync` mutably borrows each `SnapshotStore` while applying row changes.
  Snapshot stores remain owned by the root orchestration layer.
- `sync` borrows `PeerSession` values and must not clone or replace transport
  handles except through behavior explicitly allowed by the peer and transport
  contracts.
- `CopyTask`, `SkippedSubtree`, `SyncFailure`, and report values own their
  paths, metadata, and result context when they outlive an internal decision.
- Public report values are detached summaries. Mutating them has no effect on
  traversal state, scheduler state, peer files, or snapshots.
- Private child modules under `sync` may share internal records with each
  other, but sibling modules must communicate with `sync` only through
  `SyncRun`, `run`, and `SyncReport`.

## Non-API Behavior

Other modules must not depend on:

- internal module names such as traversal, excludes, classify, decision,
  dispatch, or snapshot-flow;
- traversal stack representation, recursion strategy, task spawning strategy,
  channel types, locks, or async runtime choices;
- candidate-set, vote-record, winner-record, or operation-plan structures;
- exact diagnostic wording, except where a root diagnostic contract specifies
  it;
- batching, caching, or cleanup scheduling choices;
- SQLite, transport, safe-replacement, or runtime renderer implementation
  details reached through supplied dependencies.
