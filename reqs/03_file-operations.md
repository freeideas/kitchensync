# File Operations

File copy via XFER staging, displacement to BACK, and cleanup retention.

## $REQ_FOP_001: XFER Staging Path
**Source:** ./specs/sync.md (Section: "File Copy")

File copies are staged at `<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>` on the destination peer.

## $REQ_FOP_002: Displace Existing Before Swap
**Source:** ./specs/sync.md (Section: "File Copy")

If the destination already has a file at the target path, it is displaced to `<file-parent>/.kitchensync/BACK/<timestamp>/<basename>` before the swap.

## $REQ_FOP_003: Atomic Swap from XFER
**Source:** ./specs/sync.md (Section: "File Copy")

The staged file is renamed from XFER to the final path (same-filesystem, atomic rename).

## $REQ_FOP_004: Set Mod Time After Copy
**Source:** ./specs/sync.md (Section: "File Copy")

After the swap, the destination file's modification time is set to the source file's mod_time.

## $REQ_FOP_005: Clean Up Empty XFER Directories
**Source:** ./specs/sync.md (Section: "File Copy")

Empty XFER directories are cleaned up after the copy completes.

## $REQ_FOP_006: Streaming Content Transfer
**Source:** ./specs/sync.md (Section: "File Copy")

Content is streamed, not buffered entirely in memory.

## $REQ_FOP_007: Pipelined Reader-Writer Transfer
**Source:** ./specs/sync.md (Section: "File Copy")

Each transfer spawns two concurrent tasks connected by a bounded channel: a reader task reading chunks from the source, and a writer task writing chunks to the destination. The reader and writer operate simultaneously with backpressure. A single-loop read-then-write pattern is not acceptable.

## $REQ_FOP_008: XFER Cleanup on Transfer Failure
**Source:** ./specs/sync.md (Section: "File Copy")

On transfer failure, the XFER staging file/directory for that transfer is deleted before returning connections to the pool.

## $REQ_FOP_009: BACK Displacement Path
**Source:** ./specs/sync.md (Section: "Displace to BACK")

Displacement renames the entry at `path` to `<parent>/.kitchensync/BACK/<timestamp>/<basename>`.

## $REQ_FOP_010: Directory Displacement as Single Rename
**Source:** ./specs/sync.md (Section: "Displace to BACK")

A displaced directory is moved as a single rename, preserving its entire subtree.

## $REQ_FOP_011: BACK Retention
**Source:** ./specs/sync.md (Section: "BACK Directory")

Displaced entries in BACK/ are recoverable. They are cleaned after `back-retention-days` (default: 90).

## $REQ_FOP_012: XFER Stale Cleanup
**Source:** ./specs/sync.md (Section: "XFER Staging")

Stale XFER staging directories are cleaned after `xfer-cleanup-days` (default: 2).

## $REQ_FOP_013: Kitchensync Dirs Never Synced
**Source:** ./README.md (Section: "The `.kitchensync/` Directory (in peer trees)")

`.kitchensync/` directories are never synced between peers.
