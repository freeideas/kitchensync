# 015_snapshot-row-updates-and-cleanup: Snapshot updates during and after sync

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Snapshot
Updates" and "Orphaned Snapshot Rows", `specs/database.md` sections "Schema"
and "Tombstones", and `plan/sqlite-snapshot.md`. It covers when listed,
intended-copy, completed-copy, directory-creation, confirmed-absence, and
completed-displacement events update each peer's snapshot rows; how
`last_seen` and `deleted_time` are written; how directory displacement cascades
through descendants with the recursive CTE; and how old tombstones or obsolete
rows are cleaned opportunistically.

## $REQ_IDs
- `015.1` -- When a peer listing confirms an entry is present, that peer's snapshot row for the entry records the entry's current mod_time.
- `015.2` -- When a peer listing confirms an entry is present, that peer's snapshot row for the entry records the entry's current byte_size.
- `015.3` -- When a peer listing confirms an entry is present, that peer's snapshot row for the entry records a new `last_seen` timestamp.
- `015.4` -- When a peer listing confirms an entry is present, that peer's snapshot row for the entry has `deleted_time = NULL`.
- `015.5` -- When an entry is confirmed absent on a peer with an existing non-tombstone snapshot row, that row's `deleted_time` is set to the row's previous `last_seen` value.
- `015.6` -- When an entry is confirmed absent on a peer with an existing non-tombstone snapshot row, that row's `last_seen` is not changed.
- `015.7` -- When an entry is confirmed absent on a peer with an existing tombstone row, that row is not changed.
- `015.8` -- When sync decides to push a file to a destination peer, that peer's snapshot row for the destination path records the winning entry's mod_time before the copy completes.
- `015.9` -- When sync decides to push a file to a destination peer, that peer's snapshot row for the destination path records the winning entry's byte_size before the copy completes.
- `015.10` -- When sync decides to push a file to a destination peer, that peer's snapshot row for the destination path has `deleted_time = NULL` before the copy completes.
- `015.11` -- When sync decides to push a file to a destination peer, that peer's snapshot row for the destination path keeps its existing `last_seen` value until the copy succeeds.
- `015.12` -- When sync decides to push a file to a destination peer and no destination snapshot row exists yet, the new row has `last_seen = NULL` until the copy succeeds.
- `015.13` -- After a queued file copy succeeds, the destination peer's snapshot row for that file records a new `last_seen` timestamp.
- `015.14` -- If the app exits before a queued file copy finishes, the destination peer's snapshot row for that file keeps `deleted_time = NULL`.
- `015.15` -- If the app exits before a queued file copy finishes, the destination peer's snapshot row for that file keeps its existing `last_seen` value.
- `015.16` -- After destination directory creation succeeds, the destination peer's snapshot row for that directory records the created directory's current mod_time.
- `015.17` -- After destination directory creation succeeds, the destination peer's snapshot row for that directory records `byte_size = -1`.
- `015.18` -- After destination directory creation succeeds, the destination peer's snapshot row for that directory records a new `last_seen` timestamp.
- `015.19` -- After destination directory creation succeeds, the destination peer's snapshot row for that directory has `deleted_time = NULL`.
- `015.20` -- If destination directory creation fails, the destination peer's existing snapshot row for that directory is not changed by the failed creation.
- `015.21` -- After an entry is successfully displaced to `BAK/`, that peer's snapshot row for the displaced entry has `deleted_time` set to the row's previous `last_seen` value.
- `015.22` -- If displacement to `BAK/` fails, the affected peer's existing snapshot row for that entry is not changed by the failed displacement.
- `015.23` -- After a directory is successfully displaced to `BAK/`, non-tombstone descendant snapshot rows in that same peer's displaced subtree get the displaced directory row's deletion estimate as `deleted_time`.
- `015.24` -- A directory displacement cascade does not change already-tombstoned rows in the displaced subtree.
- `015.25` -- A directory displacement cascade does not change snapshot rows outside the displaced subtree.
- `015.26` -- When the same directory is displaced on multiple peers, each peer's displacement cascade updates only that peer's own snapshot database.
- `015.27` -- Snapshot cleanup removes tombstone rows whose `deleted_time` is older than `--keep-del-days` days.
- `015.28` -- Sync decisions do not depend on eligible snapshot-cleanup rows being removed in the current run.
- `015.29` -- Orphaned non-tombstone snapshot rows that cannot be reached by a directory displacement cascade are cleaned up after their `last_seen` is older than `--keep-del-days` days.

## Notes
This file covers row mutation. It does not define the schema shape, timestamp
format, sync decisions, or transport upload of the database file.

The source permits opportunistic cleanup of known-obsolete non-tombstone rows
with old or NULL `last_seen` values. The hard requirements here mandate the
obsolete-row cleanup cases stated as required by the source text.
