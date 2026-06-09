# 018_snapshot-maintenance: Opportunistic snapshot row cleanup

## Behavior
This concern derives from `specs/multi-tree-sync.md` section "Orphaned Snapshot
Rows" and `specs/sync.md` section "Run" step 2 (the opportunistic purge timing).

It covers cleanup of snapshot rows that traversal does not otherwise visit:
removal of tombstone rows (`deleted_time IS NOT NULL`) older than
`--keep-del-days`, and removal of stale rows with `deleted_time` NULL whose
`last_seen` is older than `--keep-del-days` (or NULL) when known obsolete. It
covers the timing guarantees: this maintenance is opportunistic, may run while
visiting related directories or after copying has begun, must not delay the
first directory scan or first eligible copy, and correctness must not depend on
it finishing in the current run.

Writing and tombstoning rows during traversal is `017_snapshot-updates`.
Age-based cleanup of on-disk BAK/ and TMP/ staging is a separate filesystem
concern, `021_staging-and-displacement`.

## $REQ_IDs

- `018.1` -- A sync run removes snapshot rows with `deleted_time IS NOT NULL` whose `deleted_time` is older than `--keep-del-days` days.
- `018.2` -- A sync run keeps snapshot rows with `deleted_time IS NOT NULL` whose `deleted_time` is within `--keep-del-days` days.
- `018.3` -- A sync run removes a snapshot row with `deleted_time IS NULL` that traversal does not visit when its `last_seen` is older than `--keep-del-days` days.
- `018.4` -- Snapshot maintenance does not delay the first directory scan of a run.
- `018.5` -- Snapshot maintenance does not delay the first eligible file copy of a run.
- `018.6` -- A sync run exits 0 even when snapshot maintenance does not finish removing all eligible rows during that run.

## Notes

The spec marks removal of stale `deleted_time IS NULL` rows as optional ("may
also remove ... when those rows are known to be obsolete"), including the
`last_seen IS NULL` case. Only the firmly guaranteed case -- an unvisited row
whose `last_seen` exceeds `--keep-del-days` (stated in the displacement-cascade
cleanup guarantee) -- is asserted, to keep bullets non-hedged and externally
testable.
