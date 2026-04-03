# File Operations

File copy mechanics, TMP staging, BAK displacement, and atomic swap.

## $REQ_FOPS_001: File Copy via TMP Staging
**Source:** ./specs/algorithm.md (Section: "Operation Queue - File Copy")

File copies write to a TMP staging path `{parent}/.kitchensync/TMP/{timestamp}/{uuid}/{basename}`, then atomically rename to the final location.

## $REQ_FOPS_002: Displace Existing Before Swap
**Source:** ./specs/algorithm.md (Section: "Operation Queue - File Copy")

Before the atomic rename, if the destination file already exists, it is displaced to `{parent}/.kitchensync/BAK/{timestamp}/{basename}`.

## $REQ_FOPS_003: Set Mod-Time After Copy
**Source:** ./specs/algorithm.md (Section: "Operation Queue - File Copy")

After the atomic rename, the destination file's mod_time is set to the winning mod_time from the decision. If set_mod_time fails, a warning is logged but the copy is considered successful.

## $REQ_FOPS_004: Best-Effort Permission Copy
**Source:** ./specs/algorithm.md (Section: "Operation Queue - File Copy")

On Unix, file mode bits are copied from source to destination (best-effort). On Windows, permission copying is skipped entirely. Failures are logged at debug level and ignored.

## $REQ_FOPS_005: Displacement is Same-Filesystem Rename
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

Displacement is always a same-filesystem rename to BAK/ -- it runs inline during the walk, never queued. A displaced directory is moved as a single rename preserving its entire subtree.

## $REQ_FOPS_008: Copy Failure Cleanup
**Source:** ./specs/algorithm.md (Section: "Operation Queue - File Copy")

On transfer failure, TMP staging is cleaned up, the error is logged, and the file is skipped. It will be re-discovered on the next run.

## $REQ_FOPS_009: Concurrent File Copies
**Source:** ./specs/algorithm.md (Section: "Operation Queue")

File copies are enqueued during the walk and executed concurrently, subject to per-peer connection limits.

## $REQ_FOPS_011: BAK/TMP Cleanup by Age
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

Expired BAK/ directories are deleted when older than `--bd` days, and expired TMP/ directories when older than `--xd` days. Age is determined from the timestamp directory name, not filesystem modification time. Cleanup is skipped when the respective option is 0.

## $REQ_FOPS_010: .kitchensync Never Synced
**Source:** ./README.md (Section: "The .kitchensync/ Directory")

`.kitchensync/` directories (containing snapshot.db, BAK/, and TMP/) are never synced between peers.
