# dispatch Architecture

The `dispatch` module is the sync-private execution coordinator for outcomes
that have already been selected by the parent `sync` traversal and decision
logic. For one decided relative path, it converts the outcome into inline
operation requests, copy-task submissions, child-recursion eligibility, and
terminal copy-result consumption.

Dispatch applies the selected outcome to the peers that are active for the
current directory level, including subordinate peers as targets. It does not
classify entries, choose winners, define traversal order, choose active peer
sets, own snapshot mutation rules, implement safe replacement, retry copies,
enforce copy-slot limits, or render output.

## Responsibilities

Dispatch owns only the effect-request boundary after a path outcome is known:

- compare the selected outcome with each supplied active peer state;
- request inline displacement for active peers whose live entry conflicts with
  an absence, directory, or file outcome;
- request inline directory creation for active peers that must contain the
  selected directory;
- return the child-recursion peer set for directory outcomes, limited to peers
  whose directory already existed or whose create operation succeeded;
- submit `CopyTask` values for active peers whose file is absent or does not
  match the winning file by byte size and the five-second modification-time
  tolerance;
- treat subordinate peers as targets for creates, copies, and displacements
  without letting subordinate state alter the outcome it receives;
- notify the sync-owned snapshot-flow contract at dispatch-visible success
  points without deciding row mutation semantics itself;
- close and wait for the supplied scheduler at end-of-run dispatch time, then
  normalize terminal copy results for the parent report and snapshot-flow
  notifications.

The module should expose only sync-private helpers used by its parent. It must
not become a public API under `kitchensync::sync`, and it must not leak private
operation-plan or dispatch-result records to sibling first-layer modules.

## Inputs And Outputs

For each path-level call, dispatch receives the relative path, selected
outcome, active peer states for that directory, winning source peer and
metadata when the outcome is a file, and role information that distinguishes
contributing and subordinate peers. The parent is responsible for removing
unreachable peers, excluded paths, failed-listing subtrees, and canon-skipped
subtrees before calling dispatch.

Run-scoped dependencies are borrowed from the parent `SyncRun` or from
sync-private orchestration: run configuration, operation executor, copy
scheduler, snapshot-flow notifier, and failure/accounting collectors. Dispatch
passes dry-run context through to operation and scheduler calls but does not
perform peer-side mutation suppression itself.

Dispatch outputs requested effects and normalized accounting, not durable
independent state. Its visible consequences are operation calls, accepted
`CopyTask` submissions, child-recursion membership returned to traversal,
snapshot-flow notifications, and failure or copy-result records that the parent
folds into `SyncReport`.

## Path Effect Flow

Absence outcomes request inline displacement for every active peer that has a
live file or directory at the path. A displaced directory is never included in a
child-recursion peer set for that path.

Directory-existence outcomes first request inline displacement for active peers
that have a wrong-type file at the path. If displacement succeeds or is not
needed, dispatch requests inline directory creation for active peers that lack
the directory. The recursion peer set contains only peers whose directory
already existed or whose creation succeeded.

File-existence outcomes first request inline displacement for active peers that
have a wrong-type directory at the path. If displacement succeeds or is not
needed, dispatch submits a file copy for every active peer whose live file is
absent or differs from the winner by byte size or required modification-time
tolerance. Existing destination files that need replacement are handled by
queued copy execution and safe replacement; dispatch must not model that as
inline displacement.

All displacements and directory creates go through `OperationExecutor`.
Dispatch treats each operation result as the authority for whether later steps
for that peer may proceed. Failed displacement blocks replacement copy
submission or directory creation for that peer at that path. Failed directory
creation keeps that peer out of child recursion.

## Copy Flow

Dispatch submits eligible file-copy work as soon as the traversal reaches the
file outcome. It builds `CopyTask` values from the selected source peer,
destination peer, transport-supplied relative path spelling, and winning
metadata. It must not wait for a full-tree scan before submitting eligible
work.

The supplied scheduler owns queue representation, worker execution, retry
accounting, copy-slot limits, transfer progress, and terminal copy status.
Dispatch only submits tasks, records accepted submissions for traversal
accounting, closes copy input when the parent reaches end-of-run dispatch, waits
for scheduler completion, and consumes terminal `CopyResult` values.

Successful terminal copy results are forwarded to snapshot-flow as final copy
successes. Failed terminal copy results are normalized as copy failures for the
parent report and do not cause dispatch to retry or reinterpret the scheduler's
decision.

## Snapshot-Flow Boundary

Dispatch does not own snapshot mutation timing or row-update rules. It notifies
the sync-owned snapshot-flow owner at the event points that dispatch can
observe:

- intended file copy accepted for a destination;
- successful directory creation;
- successful displacement;
- final successful copy result.

Snapshot-flow owns how those events read or mutate `SnapshotStore` rows,
including `last_seen`, tombstones, subtree cascades, and copy-completion state.
Dispatch must not depend on SQLite schema, path hashes, local snapshot file
paths, row identifiers, or snapshot upload and SWAP recovery mechanics.

Dispatch sends no snapshot-flow notification for excluded paths, unreachable
peers, peers removed from a failed-listing subtree, peers under a canon-skipped
subtree, failed inline operations, or failed terminal copies.

## Dependency Boundaries

Dispatch depends on sync-private decision and traversal records supplied by the
parent and on the narrow contracts already visible to `sync`:

- `RunConfig` for dry-run state and run controls passed through by the parent;
- `PeerSession`, `PeerId`, and effective peer roles for target selection;
- `RelPath`, `EntryMeta`, `EntryKind`, and `Timestamp` for path and metadata
  carried into operation and copy requests;
- `OperationExecutor` and `OperationError` for inline effects;
- `CopyScheduler`, `CopyTask`, `CopyResult`, and scheduler summary values for
  queued copy work;
- sync-private snapshot-flow and report collectors for event notification and
  normalized failure accounting.

Dispatch must not call transport handles directly for mutation, inspect
snapshot stores directly, implement copy retry loops, create human-readable
diagnostic text, or reach into runtime-owned worker structures. Sibling
first-layer modules may rely only on `sync`'s public `run`, `SyncRun`, and
`SyncReport` contracts, not on dispatch internals.

## Dry-Run Behavior

Dry-run mode does not skip dispatch. Dispatch still requests the same inline
effects, submits the same eligible copy tasks, waits for terminal scheduler
results, and emits the same structured success and failure events to its parent
contracts. Operations and runtime copy execution own the read-only behavior
that prevents peer-side mutation in dry-run mode.

Dispatch also does not perform peer-side user-entry SWAP recovery before
listing or peer-side BAK/TMP retention cleanup after directory processing.
Those calls are owned by other sync orchestration paths that use
`OperationExecutor` directly.

## Error Handling

Dispatch treats ordinary operation and copy failures as path- or peer-scoped
results, not panics. It records failed inline operations for the parent report
and continues with unaffected peers and paths.

If displacement fails, dispatch treats the live entry as still present for that
peer and must not enqueue a replacement copy or include the peer in recursion in
a way that assumes success. If directory creation fails, dispatch must leave
that peer out of recursion and must not report directory creation success to
snapshot-flow. If a wrong-type directory cannot be displaced before a file
outcome, dispatch must not enqueue the file copy for that peer.

Copy-attempt failures and retry exhaustion are owned by the scheduler and copy
executor. Dispatch consumes only terminal results and leaves failed copies
discoverable for a later run through the parent snapshot contract.

## Child Modules

This scope is a leaf. It should remain a single narrow implementation module
because its work is coordination across already-owned parent and sibling
contracts; splitting it would create artificial child boundaries around private
helper records rather than independent responsibilities.
