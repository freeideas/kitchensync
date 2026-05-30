# Sync Module Architecture

## Purpose

The `sync` module owns the combined-tree traversal and reconciliation decision
engine for one prepared KitchenSync run. It lists the active peers that remain
eligible at each directory level, applies built-in and command-line excludes,
classifies live entries against per-peer snapshot rows, chooses the group
outcome for each path, updates snapshot state at the required decision points,
and requests inline operations or queued copies needed to make active peers
match the selected outcome.

`sync` is deliberately above transport, snapshot storage, safe replacement,
copy scheduling, and rendering internals. It must be testable as a recursive
combined-tree walk by using supplied peer sessions, snapshot stores, operation
executors, copy schedulers, and output sinks.

## Public Surface

The parent exposes `sync` through one traversal API for a single run. The API
accepts:

- root-owned run configuration, including dry-run mode, retry limits, excludes,
  retention settings, and copy limits;
- connected peer sessions with canonical, subordinate, or contributing role
  information;
- per-peer snapshot stores;
- an operation executor for inline peer effects;
- a copy scheduler for queued file propagation;
- diagnostic and progress sinks.

The API returns whether traversal and all queued copy work completed without
unrecovered failures, plus enough skipped-work and failure detail for the
parent to map the run result. It does not expose traversal cursors, vote
records, path groups, or child-module internals.

## Internal Split

This module should not remain a single implementation leaf. The required work
has separable traversal, classification, decision, effect dispatch, and
snapshot-timing concerns. These are private children under `sync`; they are not
visible to sibling modules. Shared contracts that later need sibling access
should move to the narrowest common ancestor instead of exposing these private
children.

### traversal

Owns the recursive pre-order combined-tree walk. For each directory it:

- reports the scanned directory to progress, using `.` for the root and
  slash-separated relative paths elsewhere;
- before listing in normal runs, asks operations to recover user-entry SWAP
  state for each peer still participating at that directory;
- starts listings on all peers participating at that directory before awaiting
  any result;
- retries each failed listing up to `RunConfig.retries_list` total attempts;
- treats failed SWAP recovery as that peer's listing failure for the same
  directory;
- applies subtree-scoped listing failure rules, including canon failure rules;
- builds candidate names from live listing names only;
- includes subordinate-only listed names only when at least one contributing
  peer remains, so they can be displaced for an absence outcome;
- removes excluded names before snapshot lookup or decision-making;
- processes candidates in deterministic case-insensitive lexicographic order,
  using the original name as the tie-breaker;
- after the directory's candidates are processed, asks operations to perform
  BAK/TMP retention cleanup in normal runs.

If a non-canon peer exhausts listing retries, `traversal` excludes that peer
from decisions, operations, recursion, and snapshot updates for that directory
subtree only. If the canon peer exhausts listing retries, it skips all
decisions, operations, copies, cleanup-driven snapshot changes, recursion, and
snapshot row changes under that subtree for every peer. If all active
contributing peers fail at a directory, it skips decisions for that subtree and
does not process subordinate-only entries there.

### excludes

Owns the local exclude predicate used by traversal. Built-in excludes are
`.kitchensync/`, `.git/`, symbolic links, special files, and other non-regular
entries omitted by transport listing or stat behavior. Command-line excludes
come from `RunConfig.excludes`; they match exact relative file paths and
directory subtree prefixes.

Excluded entries are nonexistent for the current run. They are not copied,
created, displaced, deleted, recursed into, used for decisions, or used for
snapshot updates. Existing peer contents at excluded paths are left untouched.

### classify

Owns normalized decision inputs for one candidate path. It combines live states
from active peers with snapshot rows from active contributing peers. It keeps
subordinate peer state available as target state but never lets subordinate
live entries or snapshot history vote on the group outcome.

Classification records represent live file, live directory, live absence,
wrong type, unchanged file, modified file, new file, deletion vote,
absent-unconfirmed file, no-vote, tombstone, and canon states. File
classification uses byte size and the 5-second modification-time tolerance
against snapshot rows. Directory classification ignores directory modification
time.

`classify` reads through `SnapshotStore` only. It does not know SQLite schema,
path hashing, local database paths, or snapshot file lifecycle.

### decision

Owns pure reconciliation rules. Given a classified path, peer roles, and run
configuration, it selects one group outcome:

- canon file, canon directory, or canon absence when a canon peer is active for
  the directory;
- file existence, chosen by newest contributing live file modification time
  with 5-second tolerance and size tie-breaker;
- directory existence when any contributing peer has a live directory;
- absence when deletion estimates beat live files, when contributing snapshot
  rows prove directory absence or tombstones, or when no contributing peer has
  a vote or snapshot row;
- file outcome for non-canon file-vs-directory conflicts, using normal file
  rules over contributing live files;
- skipped work when traversal failure rules prevent a safe decision.

Decision records are side-effect-free. They name the selected source metadata,
the peers that already match, the peers that need inline operations, the peers
that need queued copies, peers eligible for recursion, and snapshot
transitions that become valid only after specific observations or results.

### dispatch

Owns sync-level execution of decision records. It sends deletion and type
conflict displacements inline through operations, requests missing directory
creation inline, enqueues eligible file copies as soon as traversal finds them,
and waits for the copy scheduler to finish all queued work before `sync`
returns success.

Displacements are never placed in the file-copy queue. For directory existence,
wrong-type entries are displaced before directory creation or recursion. For
directory absence, live entries are displaced and that path is not recursed into
on that peer. For file existence, wrong-type directories are displaced before
copy planning, and copies are only enqueued for peers whose live file is absent
or does not match the winner by byte size and 5-second modification-time
tolerance.

`dispatch` does not implement safe replacement, SWAP/BAK/TMP path sequencing,
copy retry policy, copy-slot accounting, or transfer progress rendering. Those
belong to operations and runtime.

### snapshot_flow

Owns the ordering of snapshot updates requested by traversal decisions and
observed results. It maps sync events to `SnapshotStore` calls:

- confirmed-present listed files are upserted with current modification time,
  byte size, fresh `last_seen`, and `deleted_time = NULL`;
- intended destination copies are marked with the winning modification time,
  winning byte size, and `deleted_time = NULL`, without changing `last_seen`
  before the copy succeeds;
- successful copy results set destination `last_seen` to a fresh timestamp;
- failed copies leave destination `last_seen` unchanged and keep
  `deleted_time = NULL`;
- successful directory creation marks that directory present with fresh
  `last_seen`;
- confirmed absence marks existing non-tombstone rows deleted using the
  previous `last_seen` as `deleted_time`;
- existing tombstones remain unchanged;
- successful displacement marks the displaced entry deleted, and displaced
  directories request same-peer subtree cascade using the snapshot store;
- failed displacement or directory creation leaves the affected rows
  unchanged.

`snapshot_flow` never updates rows for excluded paths, unreachable peers,
peers excluded from a failed listing subtree, or any peer under a subtree
skipped because canon listing failed. Opportunistic stale-row cleanup can be
started or requested without delaying the first directory scan or first
eligible copy, and decisions must not depend on that cleanup finishing.

## Data Flow

1. The parent prepares configuration, peer sessions, snapshot stores,
   operation executor, copy scheduler, and sinks, then calls `sync`.
2. `traversal` walks directories in pre-order, maintaining the active peer set
   for each subtree and applying listing failure rules.
3. `excludes` removes built-in and command-line excluded candidates before any
   snapshot lookup.
4. `classify` combines live entry metadata and snapshot rows for each
   surviving candidate path.
5. `decision` selects the group outcome using canon, bidirectional,
   subordinate, type-conflict, deletion, and absence rules.
6. `snapshot_flow` applies snapshot updates that are valid at decision time or
   after confirmed inline results.
7. `dispatch` requests inline operations and enqueues copy work. The copy
   scheduler may run while traversal continues.
8. After traversal completes, `sync` waits for the copy scheduler's final
   results and asks `snapshot_flow` to apply successful-copy `last_seen`
   updates.
9. `sync` returns the combined traversal and copy completion result to the
   parent.

## Dependencies

`sync` consumes only explicit parent/root contracts:

- `RunConfig` for dry-run mode, retry limits, excludes, retention settings,
  and copy controls;
- `PeerSession` and peer role/state information;
- `RelPath`, `EntryMeta`, `EntryKind`, and timestamp values;
- `SnapshotStore` lookup and mutation methods;
- `OperationExecutor` for SWAP recovery, displacements, directory creation,
  retention cleanup, and copy-attempt behavior behind the scheduler;
- `CopyScheduler`, `CopyTask`, and `CopyResult`;
- `DiagnosticSink` and `ProgressSink`.

The module must not depend on CLI parser internals, peer fallback selection,
transport-specific error values, SQLite table mechanics, path hashing
implementation details, safe-replacement implementation details, copy queue
internals, terminal rendering, or sibling-module private data structures.

No separate visible ancestor API is present in this task. Contract names in
this document describe the behavioral dependencies already identified by the
parent architecture; concrete shared Rust types should be introduced at the
narrowest ancestor only when implementation jobs require them.

## Error Behavior

Directory listing failures are subtree-scoped. A non-canon peer removed from a
failed subtree may still participate elsewhere in the same run where it has not
failed, and may participate normally on later runs. A canon listing failure
blocks all decisions and mutations in that subtree because no other peer may
replace the canon outcome.

Operation failures are not retried by `sync` unless the operation API itself
defines retry behavior. `sync` reports the failure, skips dependent work for
that peer/path, and preserves snapshot rows unless the operation reported
success. A failed displacement leaves the live entry in place and prevents
recursion or replacement that would assume the displacement happened. A failed
directory creation keeps that peer out of recursion for the new directory and
leaves its directory snapshot row unchanged.

Copy retry limits and copy attempt scheduling are owned by runtime and
operations. `sync` consumes final copy results so successful copies update
destination `last_seen`, while failed copies remain discoverable on a future
run.

Diagnostics emitted by `sync` are stdout-renderable events. Formatting,
verbosity filtering, active-copy progress rendering, and process exit mapping
remain outside this module.

## Dry-Run Behavior

Dry-run mode still follows traversal, classification, decision-making, local
snapshot update, copy enqueueing, scheduler exercise, source-read, diagnostic,
and progress paths required by the run. `sync` skips peer-side SWAP recovery
before listing and peer-side BAK/TMP cleanup after directory processing in
dry-run mode.

For creates, displacements, and copies, `sync` still calls the operation and
copy-scheduler contracts with the dry-run run configuration. Operations own
suppression of peer-side mutation, and runtime owns the copy-slot behavior.

## Visibility Rules

Only the top-level sync run API and its result/error contracts are visible to
the parent. Private child modules under `sync` may share internal records with
each other, but sibling modules must communicate with `sync` only through the
parent-visible API.

If later work proves that another first-layer module needs a `sync` internal
contract, that contract should be moved to the nearest shared ancestor with a
narrow behavioral API. Sibling modules must not import private `sync`
children.
