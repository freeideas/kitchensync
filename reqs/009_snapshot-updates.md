# 009_snapshot-updates: Snapshot row updates during sync

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Snapshot Updates" and "Orphaned Snapshot Rows", `specs/sync.md` section "Run", and `specs/database.md` sections "Schema" and "Tombstones". It covers when listed entries, intended copies, completed copies, directory creations, confirmed absences, displacements, deletion cascades, and opportunistic stale-row cleanup update or preserve per-peer snapshot rows.

## $REQ_IDs
- `009.1` -- When an entry is confirmed present on a peer, that peer's `snapshot` table contains a row for the entry.
- `009.2` -- When an entry is confirmed present on a peer, that peer's snapshot row for the entry records the entry's current `mod_time`.
- `009.3` -- When an entry is confirmed present on a peer, that peer's snapshot row for the entry records the entry's current `byte_size`.
- `009.4` -- When an entry is confirmed present on a peer, that peer's snapshot row for the entry has `last_seen` set to the current sync timestamp.
- `009.5` -- When an entry is confirmed present on a peer, that peer's snapshot row for the entry has `deleted_time = NULL`.
- `009.6` -- When an entry is confirmed absent on a peer with an existing non-tombstone snapshot row, the row is retained.
- `009.7` -- When an entry is confirmed absent on a peer with an existing non-tombstone snapshot row, the row's `deleted_time` is set to the row's previous `last_seen` value.
- `009.8` -- When an entry is confirmed absent on a peer with an existing non-tombstone snapshot row, the row's `last_seen` value is not changed.
- `009.9` -- When an entry is confirmed absent on a peer with an existing tombstone row, that snapshot row is unchanged.
- `009.10` -- When a decision pushes an entry to a peer before a file copy runs, that destination peer's snapshot row records the winning entry's `mod_time`.
- `009.11` -- When a decision pushes an entry to a peer before a file copy runs, that destination peer's snapshot row records the winning entry's `byte_size`.
- `009.12` -- When a decision pushes an entry to a peer before a file copy runs, that destination peer's snapshot row has `deleted_time = NULL`.
- `009.13` -- When a decision pushes an entry to a peer before a file copy runs and no destination snapshot row exists, the new row has `last_seen = NULL`.
- `009.14` -- When a decision pushes an entry to a peer before a file copy runs and a destination snapshot row already exists, the row's `last_seen` value is not changed before the copy succeeds.
- `009.15` -- After a file copy finishes successfully, the destination peer's snapshot row for that entry has `last_seen` set to the current sync timestamp.
- `009.16` -- If an enqueued file copy does not finish, the destination peer's snapshot row for that entry keeps `last_seen` unchanged.
- `009.17` -- If an enqueued file copy does not finish, the destination peer's snapshot row for that entry has `deleted_time = NULL`.
- `009.18` -- After inline directory creation succeeds on a destination peer, that peer's snapshot row for the directory has `last_seen` set to the current sync timestamp.
- `009.19` -- When inline directory creation fails on a destination peer, that peer's existing snapshot row for the directory is unchanged.
- `009.20` -- After an entry is successfully displaced to `BAK/` on a peer, that peer's snapshot row for the entry has `deleted_time` set to the row's previous `last_seen` value.
- `009.21` -- When displacement to `BAK/` fails on a peer, that peer's existing snapshot row for the entry is unchanged.
- `009.22` -- After a directory is successfully displaced to `BAK/` on a peer, non-tombstone snapshot rows reachable through `parent_id` from the displaced directory's row in that same peer's snapshot database have `deleted_time` set to the same value as the displaced directory row.
- `009.23` -- A successful directory displacement cascade on one peer does not modify any other peer's snapshot database.
- `009.24` -- A directory displacement cascade leaves non-tombstone rows unchanged when their only `parent_id` path from the displaced directory passes through an already tombstoned row.
- `009.25` -- A directory displacement cascade leaves orphaned descendant rows unchanged when purged intermediate rows prevent reaching them through `parent_id` from the displaced directory's row.
- `009.26` -- An opportunistic snapshot cleanup pass that reaches a tombstone row with `deleted_time` older than `--keep-del-days` removes that row.
- `009.27` -- An opportunistic snapshot cleanup pass that reaches a tombstone row with `deleted_time` not older than `--keep-del-days` leaves that row in place.
- `009.28` -- With no `--keep-del-days` override, opportunistic snapshot cleanup uses 180 days as the tombstone purge threshold.
- `009.29` -- When opportunistic snapshot cleanup removes a non-tombstone snapshot row, the removed row is for an entry that no longer appears in any peer's listing and has `last_seen` older than `--keep-del-days` or `last_seen = NULL`.
- `009.30` -- Opportunistic snapshot cleanup does not remove a non-tombstone snapshot row for an entry that still appears in any peer's listing.
- `009.31` -- Opportunistic snapshot cleanup does not require all obsolete snapshot rows to be removed in the current run for sync decisions to complete correctly.
- `009.32` -- Opportunistic snapshot cleanup does not delay the first directory scan.
- `009.33` -- Opportunistic snapshot cleanup does not delay the first eligible file copy.

## Notes
This category owns semantic mutations to rows in each peer's local snapshot copy. It does not own the physical database schema or the upload/download lifecycle.
