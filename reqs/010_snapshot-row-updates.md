# 010_snapshot-row-updates: Snapshot row updates

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Snapshot
Updates", "Entry Classification", "Orphaned Snapshot Rows", "Directory
Decisions", and "Offline Peers", `specs/database.md` sections "Schema",
"Tombstones", "Path Hashing", and "Timestamps", and `specs/SCENARIOS.md`
scenarios S-02 through S-06 and S-10. It covers when snapshot rows are inserted
or updated for listed entries, intended copy destinations, completed copies,
created directories, confirmed absences, tombstones, displacement cascades,
offline or failed subtrees, opportunistic stale-row cleanup, and later
rediscovery of unfinished copy work.

## $REQ_IDs

- `010.1` -- When KitchenSync confirms a file present on a reachable peer by listing it, that peer's snapshot row for the file records the listed modification time and byte size.
- `010.2` -- When KitchenSync confirms a directory present on a reachable peer by listing it, that peer's snapshot row for the directory records the listed modification time and `byte_size = -1`.
- `010.3` -- When KitchenSync confirms an entry present on a reachable peer by listing it, that peer's snapshot row for the entry has `last_seen` set to a generated current timestamp.
- `010.4` -- When KitchenSync confirms an entry present on a reachable peer by listing it, that peer's snapshot row for the entry has `deleted_time = NULL`.
- `010.5` -- When a peer already has the winning file state with matching modification time and byte size, KitchenSync records that file as confirmed present in that peer's snapshot without copying the file.
- `010.6` -- When KitchenSync decides to copy a file to a peer, that destination peer's snapshot row records the winning file's modification time and byte size before the copy completes.
- `010.7` -- When KitchenSync decides to copy a file to a peer, that destination peer's snapshot row has `deleted_time = NULL` before the copy completes.
- `010.8` -- When KitchenSync creates a destination snapshot row for a decided file copy, that new row has `last_seen = NULL` until the copy succeeds.
- `010.9` -- When KitchenSync reuses an existing destination snapshot row for a decided file copy, that row keeps its existing `last_seen` value until the copy succeeds.
- `010.10` -- After a file copy succeeds, KitchenSync sets the destination peer's snapshot row `last_seen` to a generated current timestamp.
- `010.11` -- If a decided file copy does not complete successfully, KitchenSync leaves the destination peer's snapshot row `last_seen` unchanged.
- `010.12` -- After creating a directory on a destination peer succeeds, KitchenSync records that directory in the peer's snapshot with `byte_size = -1`, `deleted_time = NULL`, and `last_seen` set to a generated current timestamp.
- `010.13` -- If creating a directory on a destination peer fails, KitchenSync leaves that peer's existing snapshot row for the directory unchanged.
- `010.14` -- When KitchenSync confirms an entry absent on a peer and that peer has an untombstoned snapshot row for the entry, KitchenSync sets `deleted_time` to that row's existing `last_seen` value.
- `010.15` -- When KitchenSync confirms an entry absent on a peer and that peer has an untombstoned snapshot row for the entry, KitchenSync leaves that row's `last_seen` value unchanged.
- `010.16` -- When KitchenSync confirms an entry absent on a peer and that peer's snapshot row for the entry is already tombstoned, KitchenSync leaves that row unchanged.
- `010.17` -- After displacing an entry to BAK on a peer succeeds, KitchenSync sets that peer's snapshot row `deleted_time` for the displaced entry to the row's existing `last_seen` value.
- `010.18` -- After displacing a directory to BAK on a peer succeeds, KitchenSync tombstones untombstoned descendant rows under that directory in the same peer's snapshot database.
- `010.19` -- A displacement cascade writes the displaced entry's copied deletion estimate to every descendant row it tombstones.
- `010.20` -- A displacement cascade leaves already tombstoned descendant rows unchanged.
- `010.21` -- A displacement cascade leaves snapshot rows outside the displaced entry's subtree unchanged.
- `010.22` -- When the same subtree is displaced on multiple peers, KitchenSync updates each losing peer's own snapshot database and does not update one peer's snapshot database from another peer's displacement.
- `010.23` -- If displacing an entry to BAK on a peer fails, KitchenSync leaves that peer's existing snapshot row for the entry unchanged.
- `010.24` -- When live subtree evidence for a directory cannot be fully listed after the configured listing tries, KitchenSync performs no displacement-cascade or confirmed-absence snapshot updates for that directory subtree in that run.
- `010.25` -- KitchenSync does not modify snapshot rows for an unreachable peer during a run.
- `010.26` -- During opportunistic snapshot cleanup, KitchenSync removes tombstone rows whose `deleted_time` is older than `--keep-del-days` days.
- `010.27` -- During opportunistic snapshot cleanup, KitchenSync removes an untombstoned stale row only when the row is known obsolete and its `last_seen` is older than `--keep-del-days` days or `NULL`.
- `010.28` -- When a later run finds an absent file destination with an untombstoned intended-copy snapshot row whose `last_seen` is `NULL` or not more than 5 seconds newer than the source modification time, KitchenSync copies the winning file to that destination instead of tombstoning the source file.

## Notes
This category owns row-level state changes after traversal or copy events.
The SQLite file container and schema creation belong to
`004_snapshot-database-lifecycle`.
