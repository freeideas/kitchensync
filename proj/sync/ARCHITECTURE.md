# Sync Module Architecture

## Purpose

The `sync` module owns the combined-tree traversal and reconciliation decision
engine for one prepared KitchenSync run. It lists the active peers that remain
eligible at each directory level, applies built-in and command-line excludes,
classifies live entries against per-peer snapshot rows, chooses the group
outcome for each path, updates snapshot state at the required decision points,
and requests inline operations or queued copies needed to make active peers
match the selected outcome.

`sync` sits above transport, snapshot storage, safe replacement, copy
scheduling, and rendering internals. Its behavior must be testable as a
recursive combined-tree walk by supplying peer sessions, snapshot stores,
operation executors, copy schedulers, and output sinks.

This scope is not a leaf. The traversal and decision rules are large enough
that immediate child modules are needed to keep implementation jobs narrow,
but those children remain private to `sync`.

## Public Surface

The parent calls `sync` through one traversal API for a single run. The API
accepts:

- root-owned run configuration, including dry-run mode, retry limits, excludes,
  retention settings, and copy controls;
- connected peer sessions with canon, subordinate, or contributing role
  information;
- per-peer snapshot stores;
- an operation executor for inline peer effects;
- a copy scheduler for queued file propagation;
- diagnostic and progress sinks.

The API returns whether traversal and all queued copy work completed without
unrecovered failures. Skipped-work and failure detail is emitted through the
diagnostic sink rather than by exposing traversal cursors, vote records, path
groups, or child-module internals.

## Child Modules

### traversal

Owns the recursive pre-order combined-tree walk, active peer set for each
subtree, concurrent directory listing with retries, scanned-directory progress,
candidate ordering, subtree-scoped listing failure handling, and normal-run
SWAP recovery and BAK/TMP cleanup requests. It is carved because traversal
order and peer participation rules must stay independent from per-path voting
and from concrete peer mutations.

### excludes

Owns the built-in and command-line exclude predicate applied before snapshot
lookup or decision-making. It is carved because exclude behavior is small but
cross-cutting: excluded paths are treated as nonexistent for this run and must
not be copied, displaced, recursed into, or used for snapshot updates.

### classify

Owns the normalized decision inputs for one candidate path by combining live
entry metadata with per-peer snapshot rows. It is carved because file,
directory, tombstone, absent-unconfirmed, deletion-vote, canon, and subordinate
target states should be computed once before pure reconciliation rules run.

### decision

Owns side-effect-free reconciliation of a classified path into a group outcome:
canon result, file result, directory result, absence, type-conflict result, or
skipped work. It is carved so conflict rules, 5-second timestamp tolerance,
size tie-breaking, deletion estimates, and subordinate non-voting behavior can
be tested without transport, SQLite, operations, or scheduler dependencies.

### dispatch

Owns execution of selected outcomes at the sync level by requesting inline
displacements and directory creates, enqueuing eligible file copies promptly,
and waiting for queued copy work before returning. It is carved because sync
decides when effects are needed, while operations and runtime own how safe
replacement, copy retries, copy slots, and progress rendering happen.

### snapshot_flow

Owns the ordering of snapshot-store updates triggered by observations,
decisions, successful inline operations, and final copy results. It is carved
because snapshot mutation timing is part of sync correctness, but SQLite
schema, path hashing, timestamp generation, and physical snapshot lifecycle
belong to the snapshot module.

## Internal Design

`traversal` starts at the sync root and walks directories in pre-order. For
each directory it reports progress, optionally requests user-entry SWAP
recovery in normal mode, starts all peer listings before awaiting any result,
and retries each peer's listing up to `RunConfig.retries_list` total attempts.
A failed SWAP recovery is handled as that peer's listing failure for that same
directory.

Listing failures are scoped to the affected subtree. A non-canon peer that
exhausts listing retries is removed from decisions, operations, recursion, and
snapshot updates only for that directory subtree. If the canon peer exhausts
listing retries, `sync` skips decisions, operations, copies, cleanup-driven
snapshot changes, recursion, and snapshot row changes under that subtree for
all peers. If all active contributing peers fail at a directory, subordinate
only entries under that subtree are not processed.

Candidate names come only from live listings. Active contributing peer names
are included, and subordinate peer names are included only when at least one
active contributing peer remains so subordinate-only paths can be displaced for
an absence outcome. Snapshot-only rows never add names to the traversal set.
Candidates are processed in deterministic case-insensitive lexicographic order
with the original case-sensitive name as the tie-breaker, and transport
filenames are preserved when requesting operations and copies.

`excludes` removes built-in and command-line excluded candidates before any
snapshot lookup. Built-in excludes are `.kitchensync/`, `.git/`, symbolic
links, special files, and other non-regular entries omitted by transport
listing or stat behavior. Command-line excludes from `RunConfig.excludes`
match exact relative file paths and directory subtree prefixes. Existing peer
contents at excluded paths are left untouched.

`classify` gathers live state and snapshot rows from active contributing peers
for voting. Subordinate peers are retained as possible effect targets, but
their live entries and snapshot history never affect the group outcome. File
classification compares live files with non-tombstone snapshot rows by byte
size and the 5-second modification-time tolerance; directory classification
ignores directory modification time.

`decision` applies canon behavior before normal bidirectional rules. With an
active canon peer at the directory, the canon peer's live file, live directory,
or absence is authoritative. Without canon, file winners are selected from
contributing live files by newest modification time with 5-second tolerance,
then larger size among tied candidates with different sizes. Existing data wins
ties against deletion, and deletion wins only when its estimate is more than
5 seconds newer than the newest live file.

Directory existence wins when any active contributing peer has a live
directory. Directory absence wins when no contributing peer has a live
directory and contributing snapshot rows prove absence or tombstones; peers
with no directory snapshot row do not block deletion. If no contributing peer
has a live directory or any snapshot row for the path, absence is the outcome.
For non-canon file-vs-directory conflicts, a file outcome wins and the file
winner is chosen by the normal file rules over contributing live files only.

`dispatch` applies every selected group outcome to all active peers at that
directory level, including subordinate peers. Directory existence displaces
wrong-type entries inline before creating or keeping the directory, and only
peers where the directory exists or was successfully created participate in
recursion. Directory or file absence displaces any live entry inline and does
not recurse into a displaced directory on that peer.

For file existence, `dispatch` records listed source states through
`snapshot_flow`, asks operations to displace wrong-type directories inline,
and enqueues copies for peers that lack the file or whose live file does not
match the winner by byte size and the 5-second modification-time tolerance.
Displacements are never queued as copy work. File-copy work is enqueued as soon
as traversal finds eligible work, and `sync` waits for the copy scheduler to
finish all queued work before returning success.

## Snapshot Flow

`snapshot_flow` reads and mutates snapshot rows only through `SnapshotStore`.
It does not know SQLite schema, path hashing implementation details, local
temporary database paths, or physical snapshot file lifecycle.

Confirmed-present listed files are upserted with current modification time,
byte size, fresh `last_seen`, and `deleted_time = NULL`. Intended destination
copies are marked with the winning modification time, winning byte size, and
`deleted_time = NULL`, but destination `last_seen` is not changed until the
copy succeeds. Successful copy results set destination `last_seen` to a fresh
timestamp; failed copies leave `last_seen` unchanged and keep
`deleted_time = NULL`.

Successful inline directory creation marks that directory present with fresh
`last_seen`; failed creation leaves the existing row unchanged. Confirmed
absence marks an existing non-tombstone row deleted using the row's previous
`last_seen` as `deleted_time`; existing tombstones remain unchanged. Successful
displacement marks the displaced entry deleted, and displaced directories
request the snapshot store's same-peer subtree cascade. Failed displacement
leaves the affected row and descendants unchanged.

`snapshot_flow` never updates rows for excluded paths, unreachable peers, peers
excluded from a failed listing subtree, or any peer under a subtree skipped
because canon listing failed. Opportunistic stale-row cleanup may be started or
requested without delaying the first directory scan or first eligible copy, and
decisions must not depend on that cleanup finishing in the current run.

## Data Flow

1. The parent prepares configuration, peer sessions, snapshot stores,
   operation executor, copy scheduler, and sinks, then calls `sync`.
2. `traversal` walks directories in pre-order while maintaining active peer
   participation for each subtree.
3. `excludes` removes excluded candidates before snapshot lookup.
4. `classify` combines live entry metadata and snapshot rows for each
   surviving candidate path.
5. `decision` selects the group outcome using canon, bidirectional,
   subordinate, type-conflict, deletion, and absence rules.
6. `snapshot_flow` applies snapshot updates that are valid at decision time or
   after confirmed inline results.
7. `dispatch` requests inline operations and enqueues copy work. The copy
   scheduler may run while traversal continues.
8. After traversal completes, `sync` waits for final copy results and asks
   `snapshot_flow` to apply successful-copy `last_seen` updates.
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
this document describe behavioral dependencies already identified by the
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
suppression of peer-side mutation, and runtime owns copy-slot behavior.

## Visibility Rules

Only the top-level sync run API and its result contract are visible to the
parent. Private child modules under `sync` may share internal records with each
other, but sibling modules must communicate with `sync` only through the
parent-visible API.

If later work proves that another first-layer module needs a `sync` internal
contract, that contract should be moved to the nearest shared ancestor with a
narrow behavioral API. Sibling modules must not import private `sync`
children.
