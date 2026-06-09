# 017_snapshot-updates: Snapshot row updates during traversal

## Behavior
This concern derives from `specs/multi-tree-sync.md` section "Snapshot Updates"
and `specs/database.md` section "Tombstones".

It covers when and how per-peer snapshot rows change during a run. Listed state
is recorded immediately; a queued copy destination is recorded as intended state
but its `last_seen` stays unset until the copy completes; inline operations
update the row only after they succeed. It covers the specific transitions:
confirmed-present upserts mod_time, byte_size, `last_seen` = now, and clears
`deleted_time`; confirmed-absent on a live (`deleted_time` NULL) row sets
`deleted_time` to the row's current `last_seen` without touching `last_seen`, and
is idempotent once a tombstone exists; a push target upserts the winning
mod_time, byte_size, and `deleted_time` NULL without setting `last_seen`; a
completed copy then sets `last_seen` = now; a completed inline directory creation
sets `last_seen` = now; a completed displacement sets `deleted_time` to the row's
`last_seen` and cascades the deletion to descendants using the recursive CTE,
run once per peer against that peer's own database. It covers that an
interrupted copy leaves `deleted_time` NULL and `last_seen` unchanged so rule 4b
re-enqueues it next run.

The decisions that drive these updates are `011_decision-rules` and
`012_directory-and-type-decisions`. The columns themselves are
`013_snapshot-schema`. Opportunistic removal of old or orphaned rows is
`018_snapshot-maintenance`.

## $REQ_IDs

- `017.1` -- When an entry is confirmed present on a peer, that peer's snapshot row for the path records the entry's current mod_time.
- `017.2` -- When an entry is confirmed present on a peer, that peer's snapshot row records the entry's current byte_size.
- `017.3` -- When an entry is confirmed present on a peer, that peer's snapshot row sets `last_seen` to the current sync timestamp.
- `017.4` -- When an entry is confirmed present on a peer, that peer's snapshot row sets `deleted_time` to NULL.
- `017.5` -- When an entry is confirmed absent on a peer whose existing row has `deleted_time` NULL, the row's `deleted_time` is set to that row's current `last_seen` value.
- `017.6` -- When an entry is confirmed absent on a peer whose existing row has `deleted_time` NULL, the row's `last_seen` is left unchanged.
- `017.7` -- When an entry is confirmed absent on a peer whose existing row already has `deleted_time` set, the row is left unchanged.
- `017.8` -- When the decision is to push an entry to a peer, that peer's destination snapshot row records the winning entry's mod_time.
- `017.9` -- When the decision is to push an entry to a peer, that peer's destination snapshot row records the winning entry's byte_size.
- `017.10` -- When the decision is to push an entry to a peer, that peer's destination snapshot row has `deleted_time` NULL.
- `017.11` -- When the decision is to push an entry to a peer, the destination snapshot row's `last_seen` is not set by the decision, remaining NULL when no prior row exists.
- `017.12` -- After a file copy completes successfully, the destination peer's snapshot row sets `last_seen` to the current sync timestamp.
- `017.13` -- After an inline directory creation succeeds on a peer, that peer's snapshot row sets `last_seen` to the current sync timestamp.
- `017.14` -- When an inline filesystem operation fails on a peer, that peer's existing snapshot row is left unchanged.
- `017.15` -- After an entry is successfully displaced to BAK/ on a peer, that peer's snapshot row for the entry sets `deleted_time` to the row's current `last_seen` value.
- `017.16` -- After a displacement succeeds on a peer, the displaced entry's descendant rows in that peer's snapshot have `deleted_time` set.
- `017.17` -- The displacement cascade sets `deleted_time` only on rows reached as descendants of the displaced entry through `parent_id` links, leaving unrelated rows unchanged.
- `017.18` -- The displacement cascade does not overwrite `deleted_time` on descendant rows that already have `deleted_time` set.
- `017.19` -- The displacement cascade for a peer runs against that peer's own snapshot database and never against another peer's snapshot database.
- `017.20` -- When multiple peers lose the same subtree in one decision, the displacement cascade runs once per peer, each against that peer's own snapshot database, after that peer's displacement succeeds.
- `017.21` -- When a run exits before a queued copy completes, the destination snapshot row keeps `deleted_time` NULL.
- `017.22` -- When a run exits before a queued copy completes, the destination snapshot row's `last_seen` is left unchanged, remaining NULL for a first-time target.
