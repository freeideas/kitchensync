# 04_purge: Delete stale tombstones and orphaned rows at the start of a run.

## Behavior
The orchestrator calls purge once at the start of a run with a cutoff timestamp derived from its retention setting. Purge deletes two kinds of rows: expired tombstones (`deleted_time` set and older than the cutoff) and orphaned rows (rows with `deleted_time` NULL whose `last_seen` is either NULL or older than the cutoff). Recently-tombstoned rows and recently-seen rows are preserved. Derived from `SPEC.md` §"Purge" and anchored by `multi-tree-sync.md` §"Orphaned Snapshot Rows".

## $REQ_IDs
- `04.1` — Purge with cutoff `C` deletes every row where `deleted_time IS NOT NULL AND deleted_time < C`.
- `04.2` — Purge with cutoff `C` deletes every row where `deleted_time IS NULL AND (last_seen IS NULL OR last_seen < C)`.
- `04.3` — Purge preserves rows where `deleted_time IS NULL AND last_seen >= C`.
- `04.4` — Purge preserves rows where `deleted_time IS NOT NULL AND deleted_time >= C`.

## Notes
The two preserved cases (04.3, 04.4) make the cutoff a true boundary rather than a wipe; without them an implementation that deletes everything would still satisfy 04.1 and 04.2 in isolation.
