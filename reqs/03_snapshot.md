# Snapshot

Snapshot row management, path hashing, tombstones, and update semantics.

## $REQ_SNAP_001: One Row Per Path Per Peer
**Source:** ./specs/database.md (Section: "Snapshot")

The snapshot tracks per-peer state — one row per path per peer that has (or had) the entry.

## $REQ_SNAP_002: Path ID Hashing
**Source:** ./specs/database.md (Section: "Path Hashing")

The `id` field is the xxHash64 (seed 0) of the full relative path (forward slashes), base62-encoded to 11 characters (zero-padded).

## $REQ_SNAP_003: Parent ID Hashing
**Source:** ./specs/database.md (Section: "Path Hashing")

The `parent_id` field is the xxHash64 of the parent path with a trailing `/`, base62-encoded. Root entries use the hash of `/`.

## $REQ_SNAP_004: Path Hashing Conventions
**Source:** ./specs/database.md (Section: "Path Hashing")

Paths use forward slashes, no leading slash, and a trailing slash for directories and parent paths. The sync root directory itself is not tracked — only its children.

## $REQ_SNAP_006: Byte Size Convention
**Source:** ./specs/database.md (Section: "Snapshot")

`byte_size` is the file size in bytes for files, or −1 for directories.

## $REQ_SNAP_007: Last Seen Semantics
**Source:** ./specs/database.md (Section: "Snapshot")

`last_seen` is set to the current sync timestamp when the entry is confirmed present on a peer (via listing or after a completed copy). It is NULL when a push has been decided but the copy has not yet completed.

## $REQ_SNAP_008: Deleted Time Semantics
**Source:** ./specs/database.md (Section: "Snapshot")

`deleted_time` is NULL while the entry exists (or a copy is pending). It is set when the entry is confirmed absent on a peer. The value is copied from `last_seen` at the time of detection.

## $REQ_SNAP_009: Tombstone Definition
**Source:** ./specs/database.md (Section: "Tombstones")

A row with `deleted_time IS NOT NULL` is a tombstone.

## $REQ_SNAP_010: Tombstone Purge
**Source:** ./specs/database.md (Section: "Tombstones")

Tombstones are purged when `deleted_time` is older than `tombstone-retention-days` (default: 180).

## $REQ_SNAP_011: Stale Row Purge
**Source:** ./specs/multi-tree-sync.md (Section: "Orphaned Snapshot Rows")

Rows where `deleted_time IS NULL` and `last_seen` is older than `tombstone-retention-days` (or `last_seen` is NULL) are also purged during the startup purge.

## $REQ_SNAP_012: Snapshot Update — Entry Confirmed Present
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed present on a peer, the row is upserted with current mod_time, byte_size, `last_seen` set to the current sync timestamp, and `deleted_time = NULL`.

## $REQ_SNAP_013: Snapshot Update — Entry Confirmed Absent
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer with an existing row where `deleted_time` is NULL, `deleted_time` is set to the row's current `last_seen` value. `last_seen` is not updated.

## $REQ_SNAP_014: Snapshot Update — Push Decision
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When a push to a peer is decided, the destination row is upserted with the winning entry's mod_time, byte_size, and `deleted_time = NULL`. `last_seen` is not set — only confirmed presence updates it.

## $REQ_SNAP_015: Snapshot Update — Copy Completed
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

After a file copy finishes successfully, `last_seen` is set to the current sync timestamp on the destination peer's snapshot row. This is the only post-traversal snapshot update.

## $REQ_SNAP_016: Snapshot Update — Directory Creation
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

After `create_dir` and `set_mod_time` succeed on a destination peer, `last_seen` is set to the current sync timestamp on that peer's snapshot row.

## $REQ_SNAP_017: Snapshot Update — Deletion Cascade
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When a directory is displaced, `deleted_time` is set on the displaced entry and cascaded to all descendants using a recursive CTE scoped to the displaced entry's subtree via `parent_id` links.

## $REQ_SNAP_018: Snapshot Updated Before File Operations
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

Per-peer snapshot rows are updated during traversal as soon as a decision is made — before the actual file operations execute.

## $REQ_SNAP_019: Existing Tombstone Not Modified
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer with an existing row where `deleted_time` is already set, no change is made — the tombstone is already recorded.

## $REQ_SNAP_021: Deletion Cascade Value
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When a directory is displaced, `deleted_time` on the parent row is set to the row's current `last_seen`. The cascade to descendants uses this same value as the `deleted_time` parameter.
