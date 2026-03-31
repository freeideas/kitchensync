# File Operations

File copy (pipelined transfer), displacement to BAK/, TMP staging, atomic rename, and inline operations.

## $REQ_FILEOP_001: File Copies Enqueued and Concurrent
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

File copies are enqueued during the walk and executed concurrently, subject to per-peer connection limits.

## $REQ_FILEOP_002: Directory Operations Inline
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

Directory creation and displacement run inline during the walk, not enqueued.

## $REQ_FILEOP_003: TMP Staging Before Final Placement
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

A file copy writes to a TMP staging path (`{parent}/.kitchensync/TMP/{timestamp}/{uuid}/{basename}`) first, then renames to the final path.

## $REQ_FILEOP_004: Displace Existing Before Swap
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

If the destination already has a file at the target path, it is displaced to BAK/ before the atomic rename from TMP.

## $REQ_FILEOP_006: Set Mod-Time to Winner
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

After a file copy, the destination file's modification time is set to the winning mod_time from the decision.

## $REQ_FILEOP_007: Best-Effort Permission Copy
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

After a file copy, the source file's permissions are applied to the destination on a best-effort basis (failures ignored).

## $REQ_FILEOP_008: Copy Failure Cleanup
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

On copy failure, TMP staging is cleaned up, the error is logged, and the file is skipped (to be re-discovered on the next run).

## $REQ_FILEOP_009: Post-Copy Snapshot Update
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

After a file copy completes successfully, `last_seen` is set to `now` on the destination peer's snapshot row.

## $REQ_FILEOP_010: Displace to BAK
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

Displacement moves a file or directory to `{parent}/.kitchensync/BAK/{timestamp}/{basename}` via a single rename operation.

## $REQ_FILEOP_011: Directory Displacement Preserves Subtree
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

Displacing a directory is a single rename that preserves the entire subtree. Children of a displaced directory are never visited individually.

## $REQ_FILEOP_012: Pipelined Transfer
**Source:** ./specs/concurrency.md (Section: "Pipelined Transfers")

Each file transfer uses two concurrent goroutines connected by a buffered channel: a reader that reads chunks from the source and a writer that writes chunks to the destination. Reader and writer operate simultaneously — a single-loop read-then-write pattern is not acceptable.

## $REQ_FILEOP_013: Transfer Acquires Two Connections
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

A file transfer acquires one connection from the source peer's pool and one from the destination peer's pool for the duration. Both are returned on completion or failure.

## $REQ_FILEOP_014: Empty TMP Dir Cleanup
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

After a successful copy, empty parent directories in the TMP path are cleaned up.

## $REQ_FILEOP_015: Displacement Failure Handling
**Source:** ./specs/algorithm.md (Section: "Errors")

If displacement fails, the error is logged and the file is skipped (it remains in place). If part of a copy sequence, the copy is also skipped and TMP is cleaned up. For directories: the peer is excluded from recursion and tombstones are not cascaded.