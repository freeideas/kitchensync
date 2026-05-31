# snapshot_flow Architecture

`snapshot_flow` is an internal `sync` child module that owns the ordering of
snapshot-store mutations caused by traversal observations, sync decisions,
successful inline operations, and terminal copy results. It provides one narrow
place where `sync` translates confirmed facts and successful effects into
`SnapshotStore` calls.

The module is deliberately about update flow, not storage. It reads and mutates
snapshot rows only through the supplied `SnapshotStore` contract and must not
depend on SQLite tables, path hashes, row identifiers, local database paths,
snapshot download/upload lifecycle, or timestamp-generation internals.

## Responsibilities

`snapshot_flow` owns these behaviors:

- record confirmed-present file observations with live metadata, fresh
  `last_seen`, and no `deleted_time`;
- record confirmed-present directory observations with traversal-supplied
  directory metadata, fresh `last_seen`, byte size `-1`, and no
  `deleted_time`;
- record intended destination copies with the winning metadata and no
  `deleted_time`, without advancing destination `last_seen` before the copy
  succeeds;
- apply successful copy completion by advancing destination `last_seen` only
  for successful copy results;
- record successful directory creation as present with fresh `last_seen`;
- record confirmed absence by marking existing non-tombstone rows deleted using
  the row's prior `last_seen` as the deletion estimate;
- record successful displacement by marking the displaced entry deleted;
- request the same-peer displaced-directory subtree cascade from
  `SnapshotStore` after a directory displacement succeeds;
- request opportunistic stale-row cleanup with the run's deletion-retention
  setting without making sync correctness depend on cleanup completing in the
  current run.

The module also preserves the negative side of the contract: failed copy
attempts, failed displacements, and failed directory creations do not mutate
the affected snapshot rows. Excluded paths, unreachable peers, peers removed
from a failed listing subtree, and peers under a canon-listing-failed subtree
are never passed into this module for mutation.

## Internal Design

The module should expose a small private API to the rest of `sync`, centered on
a run-scoped flow object or stateless helper functions that accept explicit
peer, path, metadata, and result inputs. The API should name events in sync
terms, such as confirmed file, intended copy, copy succeeded, directory
created, absence confirmed, and displaced. It should not expose database or
row-shape concepts beyond `SnapshotRow` values returned by `SnapshotStore`.

Callers remain responsible for deciding whether an event is eligible for a
snapshot update. `snapshot_flow` assumes the caller has already applied exclude
rules, subtree-skip rules, peer role rules, operation success checks, and copy
success checks. This keeps decision-making in the traversal and reconciliation
parts of `sync` while keeping snapshot mutation order in one place.

Fresh timestamps should come from the `SnapshotStore` or from the ancestor
timestamp contract used by `SnapshotStore`; this module must not implement its
own clock or formatting. When a deletion estimate depends on an existing row's
previous `last_seen`, the module may first read the row through `SnapshotStore`
and then call the store's delete/tombstone mutation with that value.

## Data Flows

### Confirmed live entry

Traversal observes a live file or directory on a peer and passes the peer's
store, relative path, and `EntryMeta` into `snapshot_flow`. The module calls
the store's present-entry mutation so the row reflects the live metadata, a
fresh `last_seen`, and no deletion marker. Directory rows use the
traversal-supplied directory metadata and byte size `-1`.

### Intended destination copy

Decision dispatch identifies a peer that should receive a winning file and
passes the destination store, path, and winning metadata into `snapshot_flow`
before or while copy work is queued. The module records the intended file state
without changing `last_seen`, so a later failed copy does not make the
destination look observed.

### Final copy result

After the copy scheduler returns terminal results, `sync` passes each result to
`snapshot_flow`. Only successful destination copies advance destination
`last_seen`; failed copies leave the intended row state unchanged and are
reported by the surrounding `sync` failure flow.

### Successful directory creation

When an inline create-directory operation succeeds, dispatch passes the
destination store and path into `snapshot_flow`. The module marks the directory
present with fresh `last_seen`.

### Confirmed absence

When sync rules prove a path should be absent on a peer, the caller passes the
peer store and path into `snapshot_flow`. The module looks up the current row
through `SnapshotStore`; if the row is an existing non-tombstone row, it marks
the row deleted using the prior `last_seen` as the deletion estimate.

### Successful displacement

When an inline displacement succeeds, dispatch passes the peer store, path, and
displaced entry kind into `snapshot_flow`. The module marks the displaced row
deleted. If the displaced entry was a directory, it also asks `SnapshotStore`
to cascade the same-peer subtree rows for that displaced directory.

### Stale-row cleanup

The surrounding sync flow may ask `snapshot_flow` to request stale-row cleanup
for a peer store using the run's deletion-retention setting. Cleanup is
opportunistic: it must not delay the first directory scan or first eligible
copy, and traversal or decision correctness must not rely on cleanup finishing
in the current run.

## Dependencies

`snapshot_flow` may depend on these visible contracts:

- `SnapshotStore`, `SnapshotRow`, and `SnapshotEntryKind` for all snapshot
  reads and writes;
- the run deletion-retention setting when requesting stale-row cleanup;
- `RelPath` for path identity;
- `EntryMeta`, `EntryKind`, and `Timestamp` values as carried by the
  store-facing mutation APIs;
- `PeerId` only when needed for error context or caller validation inside
  `sync`;
- `CopyResult` only for applying terminal successful-copy updates.

The module must not depend on `TransportHandle`, SQLite APIs, operation
implementation details, copy scheduling internals, traversal cursors,
candidate-set structures, decision-rule helpers, local snapshot file paths, or
physical snapshot replacement behavior.

## Error Handling

Snapshot-store errors should be returned to the caller in a form the enclosing
`sync` module can convert into diagnostics and `SyncReport` failures. This
module should not render user-facing diagnostic text, choose exit status, or
retry failed storage operations unless that retry behavior is explicitly added
to the `SnapshotStore` contract.

Mutation functions should be idempotent with respect to repeated calls where
the underlying `SnapshotStore` contract allows it, but this module should not
invent compensating actions. If a store mutation fails after earlier mutations
for the same path have succeeded, the caller receives the failure and decides
how the run report represents the partial result.

Cleanup failures are non-decision-blocking unless the `SnapshotStore` reports
that the peer store can no longer be used. A failed cleanup request must not
cause snapshot-only rows to become traversal candidates or otherwise influence
group decisions.

## Ownership And Visibility

All APIs in this module are private to `sync`. The public `kitchensync::sync`
API continues to expose only `SyncRun`, `run`, reports, failures, and related
run types documented by the ancestor API.

`snapshot_flow` borrows each `SnapshotStore` only for the duration of the
specific mutation or lookup it is applying. It does not own stores, retain
references after returning, clone peer sessions, or retain copy results.

## Child Modules

This scope is a leaf. The responsibilities are a small sequence of store-facing
event handlers, and splitting them further would create artificial modules
around individual mutation calls rather than narrower independent behavior.
