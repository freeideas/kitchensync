# snapshot_flow:

## Purpose

Own the snapshot-update flow inside `sync` for one prepared KitchenSync run.
The module applies row changes that become valid after live observations,
selected outcomes, successful inline operations, and terminal copy results.

`snapshot_flow` is the sync child that keeps snapshot mutation timing
consistent. It records confirmed-present entries, intended file-copy
destinations, successful copy completion, successful directory creation,
confirmed absence, successful displacement, same-peer displaced-directory
cascades, and opportunistic stale-row cleanup requests. It uses only the
`SnapshotStore` contract supplied to `sync`.

## Responsibilities

- Accept per-peer `SnapshotStore` handles from the parent `sync` run and apply
  updates only to the store for the peer whose live observation, operation
  result, or copy result justifies the update.
- Read or preserve existing row state only through `SnapshotStore` behavior.
  The module may require the previous `last_seen` or tombstone state for
  absence and displacement updates, but it must not inspect SQLite tables,
  row identifiers, path hashes, or local database files directly.
- When a listed live entry is confirmed present on a participating peer,
  request an upsert for that peer and relative path with the observed
  modification time, observed byte size, a fresh `last_seen`, and
  `deleted_time = NULL`. Directory rows use the directory metadata supplied by
  traversal, with byte size represented as `-1`.
- When dispatch decides that a file copy should be pushed to a destination
  peer, request an intended-copy upsert for the destination peer and path with
  the winning file modification time, winning byte size, and
  `deleted_time = NULL`.
- Preserve destination `last_seen` while recording an intended copy. If no
  destination row exists, the intended-copy row has `last_seen = NULL`; if a
  row already exists, its `last_seen` is unchanged until copy success.
- After the copy scheduler reports a successful file copy, set only the
  destination row's `last_seen` to a fresh current timestamp through
  `SnapshotStore`. This is the only snapshot update that occurs after traversal
  has finished and queued copy work has reached a terminal result.
- When a queued copy fails, is abandoned, or otherwise does not report success,
  leave the destination row's `last_seen` unchanged and keep the intended-copy
  row non-tombstoned so the next run can classify the missing destination as
  absent-unconfirmed.
- After an inline directory creation succeeds on a peer, mark that directory
  confirmed present for that peer with the directory metadata and a fresh
  `last_seen`.
- If inline directory creation fails or is skipped, leave that peer's existing
  directory row unchanged.
- When a peer is confirmed absent for a path and its existing row is
  non-tombstoned, request that `SnapshotStore` retain the row, set
  `deleted_time` to the row's previous `last_seen`, and leave `last_seen`
  unchanged.
- When a peer is confirmed absent for a path and its existing row is already a
  tombstone, request no row change. Repeated absence confirmation is
  idempotent.
- When a live entry is successfully displaced to BAK on a peer, mark that
  peer's row for the displaced path deleted using the same deletion-estimate
  rule as confirmed absence: copy the row's previous `last_seen` into
  `deleted_time` and do not generate a fresh deletion timestamp.
- When a displaced entry is a directory, request the snapshot store's same-peer
  subtree cascade after the directory displacement succeeds. The cascade marks
  reachable non-tombstone descendants under that directory with the same
  deletion estimate as the displaced directory row.
- Run a displaced-directory cascade separately for each peer whose directory
  displacement succeeded. A cascade for one peer must not read or mutate any
  other peer's snapshot store.
- Leave already tombstoned descendants, descendants reachable only through an
  already tombstoned intermediate row, and orphaned descendants behind purged
  intermediate rows to `SnapshotStore` semantics and later cleanup.
- Request opportunistic stale-row cleanup using the run's deletion-retention
  setting without delaying the first directory scan or the first eligible file
  copy. Sync correctness must not depend on cleanup finishing in the current
  run.
- Treat timestamp freshness as a snapshot-store concern. Every update that
  needs a new current `last_seen` asks for a fresh timestamp-producing
  `SnapshotStore` operation; copied `deleted_time` values reuse prior
  `last_seen` values and are not fresh generated timestamps.

## Boundaries

- `snapshot_flow` owns the timing and ordering of snapshot mutations inside
  the `sync` module.
- `snapshot_flow` does not choose traversal candidates, apply excludes,
  classify peer state for voting, select winners, decide whether a path should
  exist, or determine whether a peer needs a copy, create, or displacement.
- `snapshot_flow` does not execute filesystem effects. Directory creation,
  displacement, safe copy replacement, SWAP recovery, BAK/TMP cleanup, and
  dry-run peer-side mutation suppression belong to operations and are observed
  here only as success or failure results.
- `snapshot_flow` does not enqueue file copies, count copy slots, retry copy
  attempts, stream file data, or render progress. It consumes final copy
  results supplied through the sync/runtime boundary.
- `snapshot_flow` does not own snapshot database schema, SQLite queries,
  rollback-journal behavior, path hashing, timestamp formatting or generation,
  local temporary snapshot paths, snapshot download, snapshot upload, or
  snapshot SWAP recovery. Those belong to the snapshot module behind
  `SnapshotStore`.
- `snapshot_flow` must not expose private row-update helper records as public
  sync API. Other first-layer modules interact with snapshot state only through
  the parent `sync` API and root-owned `SnapshotStore` contract.

## Error Obligations

- Never update rows for excluded paths. Excludes are applied before snapshot
  lookup, decision-making, operations, copies, recursion, or row mutation.
- Never update rows for unreachable peers. Their snapshot stores are excluded
  from the run before `sync` starts.
- Never update rows for a peer under a directory subtree where that peer was
  excluded because listing or pre-listing user-entry SWAP recovery exhausted
  retries.
- Never update any peer's rows under a subtree skipped because the canon peer
  failed listing or pre-listing user-entry SWAP recovery there.
- Preserve rows when an inline operation fails. A failed displacement leaves
  the affected row and descendants unchanged; a failed directory creation
  leaves the directory row unchanged.
- Preserve destination `last_seen` for failed copies, exhausted copies,
  cancelled copies, and copies whose result is not a success. The intended row
  may remain with `deleted_time = NULL` so the next run can rediscover and
  retry the copy.
- If a `SnapshotStore` mutation reports an error, surface the failure to the
  parent sync flow as a sync failure for that peer and path, do not apply
  dependent follow-up mutations for that same event, and do not infer that the
  peer filesystem changed differently from the operation or copy result that
  triggered the attempted update.
- Opportunistic stale-row cleanup failures are non-decision-blocking unless the
  `SnapshotStore` contract reports that the store can no longer be used. Failed
  cleanup must not cause snapshot-only rows to influence traversal candidates.
