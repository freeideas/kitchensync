# Error Handling

Error conditions, exit codes, and crash recovery.

## $REQ_ERR_001: Canon Unreachable Exits 1
**Source:** ./specs/algorithm.md (Section: "Errors")

If the canon peer is unreachable, the program exits 1.

## $REQ_ERR_002: Fewer Than Two Reachable Exits 1
**Source:** ./specs/algorithm.md (Section: "Errors")

If fewer than two peers are reachable when two or more were specified, the program exits 1.

## $REQ_ERR_003: No Reachable Peers Exits 1
**Source:** ./specs/algorithm.md (Section: "Startup")

If no peers are reachable at all, the program exits 1.

## $REQ_ERR_004: Unreachable Peer Skipped
**Source:** ./specs/algorithm.md (Section: "Errors")

An unreachable non-canon peer is skipped with a warning log. Sync continues with remaining peers.

## $REQ_ERR_005: Transfer Failure Logged and Skipped
**Source:** ./specs/algorithm.md (Section: "Errors")

A transfer failure is logged and the file is skipped. It will be re-discovered on the next run.

## $REQ_ERR_006: Successful Completion Exit 0
**Source:** ./specs/algorithm.md (Section: "Startup")

On successful completion, the program exits 0.

## $REQ_ERR_007: Crash Recovery Re-Enqueues
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

If the app exits before copies finish, destination rows have `deleted_time = NULL` and `last_seen` unchanged. The next run sees absent-unconfirmed, applies rule 4b, and re-enqueues the copy.

## $REQ_ERR_008: Argument Errors Exit 1
**Source:** ./specs/algorithm.md (Section: "Errors")

Argument errors (no peers, multiple `+`, invalid values) print an error message and help text to stdout, then exit 1.

## $REQ_ERR_009: No Snapshots No Canon Exits 1
**Source:** ./specs/algorithm.md (Section: "Errors")

In multi-peer mode, if no peer has snapshot data and no canon peer is specified, the program prints a suggestion to use `+` and exits 1.

## $REQ_ERR_010: No Contributing Peer Reachable Exits 1
**Source:** ./specs/algorithm.md (Section: "Startup")

In multi-peer mode, if no contributing peer is reachable, the program exits 1.

## $REQ_ERR_011: Displacement Failure Skipped
**Source:** ./specs/algorithm.md (Section: "Errors")

A displacement failure is logged and skipped (the file remains in place). If the displacement was part of a copy sequence, the copy is also skipped and TMP staging is cleaned up. For directory displacements, the peer is excluded from recursion into that directory and tombstones are not cascaded.

## $REQ_ERR_012: Snapshot Upload Failure Logged
**Source:** ./specs/algorithm.md (Section: "Errors")

A snapshot upload failure is logged. The TMP staging file is left for `--xd` cleanup.

## $REQ_ERR_013: Listing Error Excludes Peer From Subtree
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

If listing a directory fails for a peer, that peer is excluded from the entire subtree. The error is logged.

## $REQ_ERR_014: Root Creation Failure Marks Peer Unreachable
**Source:** ./specs/algorithm.md (Section: "Startup")

If root directory creation fails for a local (`file://`) peer with no fallback URLs, the error is logged and the peer is treated as unreachable.

## $REQ_ERR_015: Mod Time Set Failure After Copy Non-Fatal
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

If setting the modification time fails after a successful atomic rename, the copy is still considered successful. The failure is logged at warn level.
