# 04_snapshot-updates: Snapshot row maintenance during a run

## Behavior

During traversal, per-peer snapshot rows are upserted as decisions are made. `last_seen` is set only when an entry is confirmed present (live listing or completed copy/`create_dir`); `deleted_time` is set when an entry is confirmed absent on a peer that previously had it, and equals the row's prior `last_seen`. A directory deletion cascades `deleted_time` down its subtree. Tombstones survive until purged by retention. Derived from `./specs/multi-tree-sync.md` (`Snapshot Updates`, `Orphaned Snapshot Rows`) and `./specs/database.md` (`Schema`, `Tombstones`).

## $REQ_IDs
- `04.1` — A peer that observes an entry as live during traversal ends the run with that entry's snapshot row updated to current `mod_time`, current `byte_size`, `last_seen` set to the run timestamp, and `deleted_time = NULL`.
- `04.2` — A peer that observes an entry as absent (where its prior snapshot row had `deleted_time = NULL`) ends the run with `deleted_time` on that row set to the row's prior `last_seen` value, and `last_seen` unchanged.
- `04.3` — A row that already has `deleted_time` set is left unchanged when absence is confirmed again (idempotent tombstone).
- `04.4` — When a copy is decided to a peer that did not yet have the entry, the destination row is upserted with the winning `mod_time`/`byte_size` and `last_seen = NULL` until the copy completes.
- `04.5` — After a file copy completes successfully, the destination peer's row gets `last_seen` set to the run timestamp.
- `04.6` — After `create_dir` succeeds on a peer, that peer's row for the directory gets `last_seen` set to the run timestamp.
- `04.7` — When a directory is displaced on a peer, the cascade sets `deleted_time` on every descendant row (in that peer's snapshot) whose `deleted_time` was NULL.
- `04.8` — A snapshot row whose `deleted_time` is older than `--td` days is removed by the startup purge of the next run.
- `04.9` — A snapshot row with `deleted_time IS NULL` whose `last_seen` is older than `--td` days (or NULL) is removed by the startup purge of the next run.
- `04.10` — A previously-tombstoned entry that reappears live ("resurrection") has `deleted_time` cleared back to NULL on that peer's row.
