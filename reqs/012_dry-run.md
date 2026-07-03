# 012_dry-run: Dry-run behavior

## Behavior
This concern derives from `specs/sync.md` sections "Startup", "Run", "Dry
Run", "Operation Queue", "File Copy", and "Errors", `specs/database.md`
section "Database", `specs/multi-tree-sync.md` sections "SWAP Recovery During
Traversal" and "BAK/TMP Cleanup During Traversal", and `specs/SCENARIOS.md`
scenario S-08 and property "P-05: Dry Run Does Not Write Peer State". It covers
the observable read-only behavior of `--dry-run`: connecting only to existing
peer roots, downloading and locally updating temporary snapshots, listing and
planning normally, exercising copy slots and source reads, printing the dry-run
line, suppressing peer writes, skipping peer-side SWAP recovery and BAK/TMP
cleanup, and not uploading snapshots.

## $REQ_IDs
- `012.1` -- Every `--dry-run` sync prints exactly `dry run` as one stdout line before any progress line or `sync complete` line.
- `012.2` -- `--dry-run` establishes connections to reachable peer roots.
- `012.3` -- In `--dry-run`, a peer URL whose root path does not already exist is treated as unreachable for that run.
- `012.4` -- In `--dry-run`, missing peer root directories and missing parent directories remain absent.
- `012.5` -- In `--dry-run`, KitchenSync skips peer-side snapshot SWAP recovery before snapshot download.
- `012.6` -- In `--dry-run`, KitchenSync downloads each reachable peer's live `.kitchensync/snapshot.db` as the local temporary snapshot when that peer has a live snapshot.
- `012.7` -- In `--dry-run`, KitchenSync creates a new empty snapshot only as a local temporary snapshot when a reachable peer has no `.kitchensync/snapshot.db`.
- `012.8` -- In `--dry-run`, a snapshot download failure other than "not found" excludes that peer from the reachable set for the run.
- `012.9` -- `--dry-run` lists reachable peer directories for sync decisions.
- `012.10` -- `--dry-run` reads source files for queued copies.
- `012.11` -- `--dry-run` copy work acquires global copy slots subject to `--max-copies`.
- `012.12` -- `--dry-run` copy work applies the `--retries-copy` total-try limit.
- `012.13` -- `--dry-run` emits `C` and `X` progress lines under the same verbosity settings as a normal run.
- `012.14` -- `--dry-run` updates local temporary snapshot databases during traversal.
- `012.15` -- In `--dry-run`, KitchenSync does not create destination directories for planned sync content on peers.
- `012.16` -- In `--dry-run`, KitchenSync does not create `.kitchensync/TMP/`, `.kitchensync/SWAP/`, or `.kitchensync/BAK/` directories on peers.
- `012.17` -- In `--dry-run`, KitchenSync does not write destination files on peers.
- `012.18` -- In `--dry-run`, KitchenSync does not rename peer entries as part of planned copies.
- `012.19` -- In `--dry-run`, KitchenSync does not delete peer entries.
- `012.20` -- In `--dry-run`, KitchenSync does not displace peer entries to BAK.
- `012.21` -- In `--dry-run`, KitchenSync does not set file modification times on peers.
- `012.22` -- In `--dry-run`, KitchenSync does not upload updated local temporary snapshots back to peers.
- `012.23` -- In `--dry-run`, KitchenSync skips peer-side SWAP recovery during traversal.
- `012.24` -- In `--dry-run`, KitchenSync skips BAK/TMP cleanup on peers.

## Notes
This category owns the cross-cutting no-peer-write guarantee for dry runs.
The normal behavior being simulated remains owned by each operation category.
