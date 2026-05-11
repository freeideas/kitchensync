# 03_snapshot-tombstones: Snapshot updates and tombstone cascade

## Behavior

Per-peer snapshot rows are updated during traversal — before file operations execute — to reflect the decided state. Deletions cascade to descendants in the peer's snapshot. Stale rows are purged at the start of each run. Derived from `specs/multi-tree-sync.md` §"Snapshot Updates" and §"Orphaned Snapshot Rows", `specs/database.md` §"Tombstones", and `specs/sync.md` §"Run" step 1.

## $REQ_IDs
- `03.29` — When an entry is confirmed present on a peer (via listing), that peer's snapshot row is upserted with the current mod_time, byte size, `last_seen` set to the current sync timestamp, and `deleted_time` cleared.
- `03.30` — When an entry is confirmed absent on a peer where a snapshot row exists with `deleted_time` NULL, `deleted_time` is set to that row's current `last_seen` value, and `last_seen` is not updated.
- `03.31` — Repeated confirmation of absence on a row that already has `deleted_time` set leaves the existing tombstone unchanged.
- `03.32` — When a decision pushes a copy to a destination peer, that peer's snapshot row is upserted with the winning entry's mod_time, byte size, and `deleted_time = NULL`, while `last_seen` is not updated until the copy completes.
- `03.33` — After a file copy completes successfully, the destination peer's snapshot row has `last_seen` set to the current sync timestamp.
- `03.34` — After a directory is created on a destination peer, that peer's snapshot row has `last_seen` set to the current sync timestamp.
- `03.35` — When a decision displaces an entry on a peer, that peer's snapshot row for the entry has its `deleted_time` set to the row's current `last_seen`.
- `03.36` — When a directory is displaced on a peer, every descendant row in that peer's snapshot whose `deleted_time` is NULL has its `deleted_time` set to the same value used for the displaced directory; rows outside that subtree are not affected.
- `03.37` — At the start of each run, snapshot rows where `deleted_time` is not NULL and is older than `--td` days are deleted.
- `03.38` — At the start of each run, snapshot rows where `deleted_time` is NULL and `last_seen` is older than `--td` days (or `last_seen` is NULL) are deleted.

## Notes
The next run uses these row states for rule 4b absent-unconfirmed handling — see `02_decision-rules.md`. End-of-run snapshot upload mechanics are in `02_run-completion.md`.
