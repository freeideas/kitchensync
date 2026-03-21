# Sync Run

Overall sync startup sequence, run steps, operation queue, canon peer rules, and error handling.

## $REQ_RUN_001: Startup Step 1 — Resolve Config Directory
**Source:** ./specs/sync.md (Section: "Startup")

On startup, the config directory is resolved and created if it does not exist.

## $REQ_RUN_002: Startup Step 2 — Load and Merge Config
**Source:** ./specs/sync.md (Section: "Startup")

The config file is loaded (if it exists), and CLI URLs and settings are merged into it. The file is not written yet.

## $REQ_RUN_003: Startup Step 3 — Open Database
**Source:** ./specs/sync.md (Section: "Startup")

The database (`kitchensync.db`) is opened with WAL mode and the schema is executed.

## $REQ_RUN_004: Startup Step 4 — Instance Check
**Source:** ./specs/sync.md (Section: "Startup")

If another instance is already running against this config directory, print `Already running` and exit 0.

## $REQ_RUN_005: Startup Step 5 — Reconciliation Then Write Config
**Source:** ./specs/sync.md (Section: "Startup")

Peer identity reconciliation runs. On success, the merged config file is written. On failure, exit with error and leave the original config file unchanged.

## $REQ_RUN_006: Startup Step 6 — Minimum Peers
**Source:** ./specs/sync.md (Section: "Startup")

The group must have at least two peers. At least two must be reachable at runtime; with a canon peer, one reachable peer (the canon itself) is sufficient.

## $REQ_RUN_007: Startup Step 7 — First Run Without Canon Error
**Source:** ./specs/sync.md (Section: "Startup")

If no peer in the group has any snapshot data and no canon peer is designated, exit with error and suggest: "First sync? Mark the authoritative peer with a trailing !"

## $REQ_RUN_008: Startup Step 8 — Parallel Peer Connection
**Source:** ./specs/sync.md (Section: "Startup")

All peers are connected in parallel. Unreachable peers are skipped with logged warnings.

## $REQ_RUN_009: Startup Step 9 — Canon Unreachable Error
**Source:** ./specs/sync.md (Section: "Startup")

If the canon peer is unreachable, exit with error.

## $REQ_RUN_010: Canon CLI Not Persisted
**Source:** ./specs/sync.md (Section: "Canon Peer")

The `!` suffix on the CLI marks a peer as canon for this run only. It is not persisted to the config file.

## $REQ_RUN_011: Canon Config File Permanent
**Source:** ./specs/sync.md (Section: "Canon Peer")

`"canon": true` in the config file sets permanent canon. The user must edit the config file to set this.

## $REQ_RUN_014: Run Step 1 — Purge Tombstones and Logs
**Source:** ./specs/sync.md (Section: "Run")

At the start of a run, snapshot tombstones older than `tombstone-retention-days` are purged. Stale rows with `deleted_time IS NULL` and `last_seen` older than `tombstone-retention-days` (or NULL) are also purged. Expired log entries are purged.

## $REQ_RUN_015: Run Step 2 — Combined-Tree Walk
**Source:** ./specs/sync.md (Section: "Run")

The combined-tree walk is executed: directory creation and displacement inline, file copies enqueued, snapshot updated during traversal, per-peer concurrency limits enforced.

## $REQ_RUN_016: Run Step 3 — Wait for Copies
**Source:** ./specs/sync.md (Section: "Run")

After the tree walk, all enqueued file copies are waited on to complete.

## $REQ_RUN_017: Run Step 4 — Disconnect and Exit
**Source:** ./specs/sync.md (Section: "Run")

After all copies complete, all peers are disconnected, completion is logged, and the process exits.

## $REQ_RUN_020: Error — Config Errors Exit 1
**Source:** ./specs/sync.md (Section: "Errors")

Config errors (invalid settings, multiple canon peers, URLs from different groups, no canon and no snapshot history) cause a print to stdout and exit 1.

## $REQ_RUN_022: Error — Transfer Failure Skipped
**Source:** ./specs/sync.md (Section: "Errors")

Transfer failures are logged. The file is skipped and will be re-discovered on the next run.

## $REQ_RUN_023: Error — Displacement Failure Skipped
**Source:** ./specs/sync.md (Section: "Errors")

If displacement to BACK/ fails, the error is logged and the displacement is skipped (file remains in place). If the displacement was part of a file copy sequence, the copy is also skipped and the XFER staging file is cleaned up.

## $REQ_RUN_024: Error — XFER Staging Failure
**Source:** ./specs/sync.md (Section: "Errors")

XFER staging failures (cannot create staging directory or write staging file) are treated as transfer failures.

## $REQ_RUN_025: Case Sensitivity Preservation
**Source:** ./specs/sync.md (Section: "Case Sensitivity")

Filenames are preserved exactly as the filesystem reports them.
