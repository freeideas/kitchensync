# Snapshot Updates

Per-peer snapshot row management during traversal and tombstone handling.

## $REQ_SNAP_001: Pre-Operation Snapshot Update
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

Per-peer snapshot rows are updated during traversal as soon as a decision is made — before actual file operations execute. The snapshot reflects the decided state, not what has physically happened yet.

## $REQ_SNAP_002: Entry Confirmed Present
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed present on a peer, its snapshot row is upserted with current mod_time, byte_size, `last_seen` set to the current sync timestamp, and `deleted_time = NULL`.

## $REQ_SNAP_003: Entry Confirmed Absent - New Tombstone
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer with an existing row where `deleted_time` is NULL, `deleted_time` is set to the row's current `last_seen` value. `last_seen` is not updated.

## $REQ_SNAP_004: Entry Confirmed Absent - Existing Tombstone
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer with an existing row where `deleted_time` is already set, no change is made.

## $REQ_SNAP_005: Push Decision Snapshot
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When a push to a peer is decided, the destination peer's snapshot row is upserted with the winning entry's mod_time, byte_size, and `deleted_time = NULL`. `last_seen` is not updated — it is only set when the entry is confirmed present. If no row exists yet, `last_seen` is NULL.

## $REQ_SNAP_006: Copy Completed Last Seen Update
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

After a file copy finishes successfully, `last_seen` is set to the current sync timestamp on the destination peer's snapshot row. This is the only post-traversal snapshot update.

## $REQ_SNAP_007: Directory Creation Last Seen Update
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

After `create_dir` and `set_mod_time` succeed on a destination peer, `last_seen` is set to the current sync timestamp on that peer's snapshot row. Directory creation is confirmed in one step (unlike file copies).

## $REQ_SNAP_008: Delete Decision Snapshot Cascade
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When a deletion is decided for a peer, `deleted_time` is set to the row's current `last_seen` on that peer's row. Then a recursive CTE cascades to all descendants, setting `deleted_time` on rows where `deleted_time IS NULL` for that peer and entry subtree.

## $REQ_SNAP_009: Incomplete Copy Recovery
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

If the app exits before copies finish, the destination row has `deleted_time = NULL` and `last_seen` unchanged. The next run sees the entry as absent-unconfirmed and applies rule 4b, re-enqueuing the copy.

## $REQ_SNAP_010: Tombstone Definition
**Source:** ./specs/database.md (Section: "Tombstones")

A snapshot row with `deleted_time IS NOT NULL` is a tombstone.
