# dispatch Module API

## Export Policy

The `dispatch` module has no public API outside the parent `sync` module.
Sibling first-layer modules and crate users must not depend on
`kitchensync::sync::dispatch` items.

Dispatch is a sync-private leaf because it only coordinates effects for
outcomes already selected by `sync`. Public product behavior remains exposed
through the parent `sync` contracts, including `run`, `SyncRun`, and
`SyncReport`, plus the root-owned shared contracts for operations, runtime,
snapshot, peers, paths, and metadata.

Rust visibility for dispatch items should be private by default. Any callable
entry point needed by parent sync orchestration may be `pub(super)` or
`pub(crate)` only if required by the module layout; it must not be re-exported
from `sync`.

## Sync-Private Entry Points

Parent sync orchestration may rely on dispatch to provide two conceptual
operations:

- Apply one decided path outcome to the supplied active peers.
- Finish end-of-run copy dispatch by closing the scheduler, waiting for
  accepted copy work, and normalizing terminal copy results.

These entry points are implementation-private Rust functions. Their exact
names and helper records are not stable outside `sync`, but their behavior must
preserve the contract below.

### Path Outcome Dispatch

A path-dispatch function receives borrowed run-scoped dependencies from the
parent sync run:

- run configuration, including dry-run state;
- operation executor for inline displacements and directory creation;
- copy scheduler for accepted file-copy work;
- snapshot-flow notifier owned by sync;
- report or accounting collectors owned by sync.

It also receives the per-path inputs selected by parent traversal and decision
logic:

- `RelPath` using the transport-supplied relative path spelling;
- selected outcome: absence, directory exists, or file exists;
- active peer set for the current directory subtree;
- each active peer's live state at the path;
- peer role information distinguishing contributing peers from subordinate
  target-only peers;
- winning source peer and winning `EntryMeta` when the outcome is file
  existence.

The function returns the child-recursion peer set for directory-existence
outcomes and reports path- or peer-scoped operation failures through the
parent's sync-private accounting channel. For non-directory outcomes, no child
recursion peers are returned.

### Finish Dispatch

An end-of-run dispatch function closes copy submission, waits for the supplied
`CopyScheduler` to finish all accepted work, consumes terminal `CopyResult`
values, forwards final successful copies to snapshot-flow, and returns
normalized copy success and failure information for `SyncReport` assembly.

Dispatch does not retry failed copies or reinterpret scheduler retry decisions.

## Effect Rules

Dispatch applies outcomes only to peers supplied by parent sync as active for
the current subtree. It must not send operation requests, copy tasks,
recursion membership, or snapshot-flow events for unreachable peers, excluded
paths, failed-listing subtrees, or canon-skipped subtrees.

Subordinate peers are targets only. Dispatch may request creates, copies, and
displacements for subordinate peers, but subordinate state must not change the
selected outcome it receives.

For absence outcomes, dispatch requests inline displacement for every active
peer with a live file or directory at the path. Displaced directories are not
included in child recursion.

For directory-existence outcomes, dispatch first requests inline displacement
for any active peer with a wrong-type file. If displacement succeeds or is not
needed, it requests inline directory creation for active peers lacking the
directory. Only peers whose directory already existed or whose creation
succeeded are returned for child recursion.

For file-existence outcomes, dispatch first requests inline displacement for
any active peer with a wrong-type directory. If displacement succeeds or is not
needed, it submits a `CopyTask` for each active peer whose file is absent or
does not match the winning file by byte size and the required five-second
modification-time tolerance. Matching files receive no copy task.

Displacement is always an inline operation request. Dispatch must not represent
deletion or type-conflict displacement as queued file-copy work.

## Snapshot-Flow Events

Dispatch may notify the sync-owned snapshot-flow contract only for events it
can observe:

- intended file copy accepted by the scheduler;
- successful directory creation;
- successful displacement;
- final successful copy result.

Dispatch must not define snapshot row mutation semantics and must not depend on
SQLite schema details, row identifiers, path hashes, local snapshot file paths,
or snapshot upload and SWAP recovery mechanics.

No snapshot-flow event is emitted for failed inline operations, failed terminal
copies, excluded paths, unreachable peers, failed-listing subtrees, or
canon-skipped subtrees.

## Error And Ownership Rules

Operation and copy failures are peer- or path-scoped results, not panics. A
failed displacement leaves the live entry treated as still present for that
peer, and dispatch must not enqueue a replacement copy or include that peer in
recursion based on the failed displacement. A failed directory creation keeps
that peer out of recursion and emits no directory-created snapshot-flow event.

Dispatch borrows run-scoped dependencies from parent sync orchestration and
does not own transport handles, snapshot stores, scheduler internals, worker
queues, locks, channels, or async runtimes. It must not call transport mutation
methods directly, inspect snapshot stores directly, render diagnostics, enforce
copy-slot limits, or perform copy retry loops.

Dry-run mode is passed through to operation and scheduler calls. Dispatch still
requests the same creates, displacements, and copy submissions that a normal
run would require; peer-side mutation suppression belongs to operations and
runtime copy execution.
