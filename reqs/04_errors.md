# Error Handling

Observable error behavior for configuration errors, transfer failures, and displacement failures.

## $REQ_ERR_001: Config Error Exit
**Source:** ./specs/sync.md (Section: "Errors")

Configuration errors (bad JSON5, unknown peer in `--canon`, missing file) are printed to stdout, and the application exits with code 1.

## $REQ_ERR_002: Unreachable Peer Continues
**Source:** ./specs/sync.md (Section: "Errors")

When a peer is unreachable, it is skipped with a logged warning. The sync continues with remaining peers.

## $REQ_ERR_003: Transfer Failure Logged and Skipped
**Source:** ./specs/sync.md (Section: "Errors")

When a file transfer fails, the error is logged and the file is skipped. It will be re-discovered on the next run.

## $REQ_ERR_004: Displacement Failure Logged and Skipped
**Source:** ./specs/sync.md (Section: "Errors")

When a displacement to BACK/ fails (cannot rename), the error is logged and the displacement is skipped — the file remains in place.

## $REQ_ERR_005: Displacement Failure Blocks Copy
**Source:** ./specs/sync.md (Section: "Errors")

If a displacement failure was part of a file copy sequence, the copy is also skipped and the XFER staging file is cleaned up.

## $REQ_ERR_006: XFER Staging Failure
**Source:** ./specs/sync.md (Section: "Errors")

If XFER staging fails (cannot create staging directory or write staging file), it is treated as a transfer failure.