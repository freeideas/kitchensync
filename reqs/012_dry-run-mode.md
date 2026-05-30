# 012_dry-run-mode: Dry-run read-only execution

## Behavior
This concern derives from `specs/sync.md` sections "Dry Run", "Startup", "Run", "File Copy", and "Errors", `specs/database.md` section "Database", and `specs/multi-tree-sync.md` sections "SWAP Recovery During Traversal", "BAK/TMP Cleanup During Traversal", and "Snapshot Updates". It covers the observable `--dry-run` contract: realistic planning and reads, local temporary snapshot updates, copy-slot and source-read exercise, no peer-side creation or mutation, no peer-side SWAP recovery or BAK/TMP cleanup, unreachable treatment of missing roots, skipped snapshot upload, and required dry-run output.

## $REQ_IDs
- `012.1` -- `kitchensync --dry-run` treats a peer URL whose root path or required root parent path does not already exist as unreachable for that run.
- `012.2` -- `kitchensync --dry-run` leaves missing peer root paths and missing peer root parent paths uncreated.
- `012.3` -- `kitchensync --dry-run` skips peer-side snapshot SWAP recovery during startup.
- `012.4` -- `kitchensync --dry-run` downloads each reachable peer's live `.kitchensync/snapshot.db` as-is when that file exists.
- `012.5` -- `kitchensync --dry-run` creates a new local temporary snapshot database for each reachable peer whose live `.kitchensync/snapshot.db` is not found.
- `012.6` -- `kitchensync --dry-run` updates local temporary snapshot databases during traversal.
- `012.7` -- `kitchensync --dry-run` does not upload updated local temporary snapshot databases back to peers.
- `012.8` -- `kitchensync --dry-run` connects to peers before making sync decisions.
- `012.9` -- `kitchensync --dry-run` lists peer directories during the combined-tree walk.
- `012.10` -- `kitchensync --dry-run` enqueues file-copy work for files that would be copied in a normal run.
- `012.11` -- `kitchensync --dry-run` enforces `--max-copies` as the maximum number of dry-run transfers that hold copy slots at the same time.
- `012.12` -- `kitchensync --dry-run` reads source file contents for queued copy work.
- `012.13` -- `kitchensync --dry-run` applies `--retries-copy` total-try behavior to dry-run copy work.
- `012.14` -- `kitchensync --dry-run` creates no destination directories through any `file://` or `sftp://` peer URL.
- `012.15` -- `kitchensync --dry-run` creates no TMP, SWAP, or BAK directories through any `file://` or `sftp://` peer URL.
- `012.16` -- `kitchensync --dry-run` writes no destination files through any `file://` or `sftp://` peer URL.
- `012.17` -- `kitchensync --dry-run` displaces no destination files or directories to BAK through any `file://` or `sftp://` peer URL.
- `012.18` -- `kitchensync --dry-run` deletes no destination files or directories through any `file://` or `sftp://` peer URL.
- `012.19` -- `kitchensync --dry-run` sets no file or directory modification times through any `file://` or `sftp://` peer URL.
- `012.20` -- `kitchensync --dry-run` skips peer-side user-file SWAP recovery during traversal.
- `012.21` -- `kitchensync --dry-run` skips peer-side BAK cleanup during traversal.
- `012.22` -- `kitchensync --dry-run` skips peer-side TMP cleanup during traversal.
- `012.23` -- `kitchensync --dry-run` prints the phrase `dry run` at least once on stdout.

## Notes
This category owns dry-run deviations from normal execution. Normal-mode behavior remains in the domain-specific categories.
