# 03_tombstones: Mark single entries absent and cascade tombstones over a displaced subtree.

## Behavior
When traversal confirms an entry is absent on the peer, the snapshot records a tombstone on that row by setting `deleted_time` to the row's current `last_seen` value — the time of the last confirmed sighting becomes the deletion time. Repeated mark-absent calls and mark-absent on missing rows are no-ops. When a directory is displaced wholesale, a cascade walks down the `parent_id` graph from the directory's id and tombstones every still-live descendant with a single supplied timestamp, without touching descendants of other tombstoned ancestors that aren't descendants of *this* directory. Derived from `SPEC.md` §"Row operations" and anchored by `database.md` §"Tombstones" and `multi-tree-sync.md` §"Snapshot Updates".

## $REQ_IDs
- `03.8` — Mark-absent on a path that has no row in the snapshot does nothing (the snapshot is unchanged).
- `03.9` — Mark-absent on a path whose row has `deleted_time IS NULL` sets that row's `deleted_time` to the row's current `last_seen` value.
- `03.10` — Mark-absent leaves the row's `last_seen` unchanged.
- `03.11` — Mark-absent on a row whose `deleted_time` is already set leaves the row unchanged (idempotent).
- `03.12` — Cascade-tombstone for `(id, ts)` sets `deleted_time` to `ts` on every descendant row reachable through `parent_id` links from `id` whose `deleted_time` was NULL.
- `03.13` — Cascade-tombstone preserves the existing `deleted_time` of any already-tombstoned descendant (does not overwrite).
- `03.14` — Cascade-tombstone affects only true descendants of the supplied id — rows that are not reachable from `id` via `parent_id` links are untouched, even if they happen to share an unrelated tombstoned ancestor.

## Notes
The cascade is the row-side counterpart of the orchestrator's "directory displacement" action. The recursive walk goes down the `parent_id` graph; this is what makes 03.14 hold.
