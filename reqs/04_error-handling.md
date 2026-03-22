# Error Handling

Error conditions and their specified behavior during sync operations.

## $REQ_ERR_001: Argument Errors
**Source:** ./specs/sync.md (Section: "Startup"), ./specs/help.md (Section: top-level)

Argument validation errors (fewer than two peers, multiple `+` peers, unrecognized flags, invalid option values) print the error message followed by the help text to stdout and exit 1.

## $REQ_ERR_002: No Snapshots and No Canon
**Source:** ./specs/sync.md (Section: "Errors")

When no peer has snapshots and no canon is designated, the program prints a suggestion to use `+` and exits 1.

## $REQ_ERR_011: No Contributing Peer Reachable
**Source:** ./specs/sync.md (Section: "Startup")

If no contributing (non-subordinate) peer is reachable after auto-subordination of snapshotless peers, the program exits with error.

## $REQ_ERR_003: Unreachable Peer Skipped
**Source:** ./specs/sync.md (Section: "Errors")

An unreachable peer is skipped with a warning logged. Sync continues with remaining peers.

## $REQ_ERR_004: Canon Peer Unreachable
**Source:** ./specs/sync.md (Section: "Errors")

If the canon peer is unreachable, the program exits with error.

## $REQ_ERR_005: Fewer Than Two Reachable Peers
**Source:** ./specs/sync.md (Section: "Errors")

If fewer than two peers are reachable, the program exits with error.

## $REQ_ERR_006: Transfer Failure
**Source:** ./specs/sync.md (Section: "Errors")

On transfer failure, the error is logged and the file is skipped. It will be re-discovered on the next run.

## $REQ_ERR_007: Displacement Failure
**Source:** ./specs/sync.md (Section: "Errors")

If displacement (rename to BAK/) fails, the error is logged and the displacement is skipped (file remains in place). If the displacement was part of a file copy sequence, the copy is also skipped (TMP staging file is cleaned up).

## $REQ_ERR_008: TMP Staging Failure
**Source:** ./specs/sync.md (Section: "Errors")

If TMP staging fails (cannot create staging directory or write staging file), it is treated as a transfer failure.

## $REQ_ERR_009: Snapshot Upload Failure
**Source:** ./specs/sync.md (Section: "Errors")

On snapshot upload failure, the error is logged. The peer's snapshot will be stale on the next run, leading to redundant but correct copies.

## $REQ_ERR_010: Pool Connection Failure During Transfer
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

A pool connection failure during a transfer is a transfer failure.

## $REQ_ERR_012: Listing Error Excludes Peer From Subtree
**Source:** ./specs/multi-tree-sync.md (Section: "Listing errors")

If `list_directory` fails for a reachable peer at a specific path, that peer is excluded from decisions for that directory and its entire subtree. The error is logged. The peer's snapshot rows for that subtree are not modified.
