# Snapshot Lifecycle

Snapshot download, updates during traversal, checkpoints, and upload.

## $REQ_SNAP_001: Snapshot Download to Local Temp
**Source:** ./specs/database.md (Section: top)

At the start of a run, each peer's `snapshot.db` is downloaded to a local temporary directory. All reads and writes happen against this local copy.

## $REQ_SNAP_002: New Snapshot for Missing Database
**Source:** ./specs/database.md (Section: top)

If a peer has no existing `snapshot.db`, a new one is created locally with the schema and sentinel row.

## $REQ_SNAP_003: WAL Checkpoint Before Upload
**Source:** ./specs/database.md (Section: top)

Before uploading `snapshot.db`, a WAL checkpoint must be performed: `PRAGMA wal_checkpoint(TRUNCATE)`. Without this, the uploaded file is an empty stub.

## $REQ_SNAP_004: Snapshot Upload via TMP Staging
**Source:** ./specs/algorithm.md (Section: "Startup")

After sync completes, updated snapshots are uploaded to each peer via TMP staging with atomic rename to `.kitchensync/snapshot.db`.

## $REQ_SNAP_005: Snapshot Updated During Traversal
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

Snapshots are updated as soon as a decision is made -- before file copies execute. The snapshot reflects decided state, not physical state.

## $REQ_SNAP_006: Present Entry Snapshot Update
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When an entry is confirmed present on a peer, its snapshot row is upserted with current mod_time, byte_size, `last_seen=now`, and `deleted_time=NULL`.

## $REQ_SNAP_007: Push Target Snapshot Update
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When a copy is enqueued to a peer, the snapshot row is upserted with the winner's mod_time and byte_size, `deleted_time=NULL`, but `last_seen` is NOT set (only set after copy completes).

## $REQ_SNAP_008: Post-Copy Last-Seen Update
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

After a file copy completes successfully, `last_seen` is set to now on the destination peer's snapshot row. This is the only post-traversal snapshot update.

## $REQ_SNAP_009: Snapshot Checkpoints During Sync
**Source:** ./specs/algorithm.md (Section: "Snapshot Checkpoints")

During long syncs, snapshots are periodically uploaded to peers at the `--si` interval (default: 30 minutes). After each completed file copy, if the timer exceeds `--si` minutes, all peers' snapshots are uploaded. The upload uses each peer's listing connection, not the transfer pool.

## $REQ_SNAP_011: Absent Entry Tombstone Creation
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer and the snapshot row has `deleted_time = NULL`, `deleted_time` is set to the row's `last_seen` value. If `deleted_time` is already set, no change is made.

## $REQ_SNAP_012: Cascade Tombstones on Directory Displacement
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When a directory is displaced from a peer, `deleted_time` is set on the directory's snapshot row and recursively on all descendant rows that have `deleted_time = NULL`. The `deleted_time` value used for all rows is the displaced directory's own `last_seen`.
