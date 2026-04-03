# Dry Run Mode

Preview what a sync would do without making changes.

## $REQ_DRY_001: Dry Run Flag
**Source:** ./specs/algorithm.md (Section: "Dry Run Mode")

When `--dry-run` or `-n` is specified, the sync runs normally through decision-making but skips all mutating operations.

## $REQ_DRY_002: Decisions Still Made
**Source:** ./specs/algorithm.md (Section: "Dry Run Mode")

In dry-run mode, peer connections, snapshot downloads, directory tree walks, and all sync decisions still happen normally.

## $REQ_DRY_003: Operations Logged But Not Executed
**Source:** ./specs/algorithm.md (Section: "Dry Run Mode")

Copy and delete operations are logged (`C <path>` and `X <path>`) for every operation that would happen, but file copies, displacements, directory creation/deletion, and snapshot uploads are all skipped.

## $REQ_DRY_004: BAK/TMP Cleanup Skipped
**Source:** ./specs/algorithm.md (Section: "Dry Run Mode")

BAK/ and TMP/ cleanup is skipped in dry-run mode.

## $REQ_DRY_005: Snapshot Checkpoints Skipped
**Source:** ./specs/algorithm.md (Section: "Snapshot Checkpoints")

Snapshot checkpoints are skipped in dry-run mode (no mutations).

## $REQ_DRY_006: Dry Run with Watch
**Source:** ./specs/watch.md (Section: "Interaction with Other Flags")

`--dry-run` with `--watch` performs the initial sync in dry-run mode, then watches and logs what would happen for each change without executing.
