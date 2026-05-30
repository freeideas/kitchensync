# 011_displacement-and-staging-cleanup: Displacement, SWAP recovery, and retention cleanup

## Behavior
This concern derives from `specs/sync.md` sections "Displace to BAK", "TMP Staging", "SWAP Directory", "BAK Directory", and "Errors", plus `specs/multi-tree-sync.md` sections "SWAP Recovery During Traversal", "BAK/TMP Cleanup During Traversal", "Directory deletion", and "All displacement is inline". It covers inline displacement of files and directories to nearby BAK locations, per-directory user-file SWAP recovery before listing, TMP staging path rules, BAK and TMP retention cleanup, failure behavior for displacement and staging cleanup, and the rule that SWAP directories are recovered rather than age-purged.

## $REQ_IDs
- `011.1` -- KitchenSync executes every deletion and type-conflict displacement during the combined-tree walk.
- `011.2` -- KitchenSync does not place deletion or type-conflict displacement work in the file-copy operation queue.
- `011.3` -- Before displacing an entry, KitchenSync creates `<parent>/.kitchensync/BAK/<timestamp>/` and any missing parent directories when they do not already exist.
- `011.4` -- Displacing `<parent>/<basename>` renames the entry to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `011.5` -- BAK directories for displaced entries are created under the displaced entry's parent directory rather than aggregated at the sync root.
- `011.6` -- Displacing a directory moves the directory as a single rename with its subtree preserved under BAK.
- `011.7` -- KitchenSync does not recurse into a directory on a peer after deciding to displace that directory on that peer.
- `011.8` -- Only peers keeping a directory participate in recursive traversal inside that directory.
- `011.9` -- TMP staging for temporary metadata and cleanup work is placed inside `.kitchensync/`.
- `011.10` -- TMP staging uses a distinct UUID per transfer.
- `011.11` -- For user entry target `<parent>/<basename>`, KitchenSync uses `<parent>/.kitchensync/SWAP/<encoded-basename>/new` as the SWAP `new` path.
- `011.12` -- For user entry target `<parent>/<basename>`, KitchenSync uses `<parent>/.kitchensync/SWAP/<encoded-basename>/old` as the SWAP `old` path.
- `011.13` -- `<encoded-basename>` is the target basename percent-encoded when needed so it can be used as one path segment on every supported transport.
- `011.14` -- In a normal run, before listing a directory for sync decisions, KitchenSync checks each peer for `.kitchensync/SWAP/` at that directory level.
- `011.15` -- In a normal run, KitchenSync recovers every direct child swap directory under `.kitchensync/SWAP/` before listing that directory's live entries for sync decisions.
- `011.16` -- In `--dry-run`, KitchenSync skips peer-side SWAP recovery during traversal.
- `011.17` -- When user-entry SWAP recovery finds `old` and the target present, it moves `old` to BAK.
- `011.18` -- When user-entry SWAP recovery finds `old`, `new`, and no target, it renames `new` to the target.
- `011.19` -- When user-entry SWAP recovery finds `old`, `new`, and no target, it moves `old` to BAK.
- `011.20` -- When user-entry SWAP recovery finds `old` present and both `new` and the target missing, it renames `old` back to the target.
- `011.21` -- When user-entry SWAP recovery finds `new` and the target present with no `old`, it deletes `new`.
- `011.22` -- When user-entry SWAP recovery finds `new` present and both `old` and the target missing, it renames `new` to the target.
- `011.23` -- After successful user-entry SWAP recovery, KitchenSync removes the empty swap directory.
- `011.24` -- If recovery for a user-entry swap directory fails, KitchenSync treats that peer's listing for the current directory as failed.
- `011.25` -- If recovery for a user-entry swap directory fails, KitchenSync excludes that peer from sync decisions for the current directory subtree.
- `011.26` -- If recovery for a user-entry swap directory fails, KitchenSync does not modify that peer's snapshot rows for the current directory subtree.
- `011.27` -- If user-entry SWAP recovery fails for the canon peer at a directory, KitchenSync makes no peer file changes under that directory subtree during that run.
- `011.28` -- In a normal run, after processing the union of entry names at a directory level, KitchenSync checks each peer for `.kitchensync/` at the current path for BAK/TMP cleanup.
- `011.29` -- BAK/TMP cleanup considers `.kitchensync/` metadata directories even though `.kitchensync/` is excluded from sync entry decisions.
- `011.30` -- `.kitchensync/BAK/<timestamp>/` entries older than `--keep-bak-days` days are purged during BAK/TMP cleanup.
- `011.31` -- With no `--keep-bak-days` override, BAK cleanup purges `.kitchensync/BAK/<timestamp>/` entries older than 90 days.
- `011.32` -- `.kitchensync/TMP/<timestamp>/` entries older than `--keep-tmp-days` days are purged during BAK/TMP cleanup.
- `011.33` -- With no `--keep-tmp-days` override, TMP cleanup purges `.kitchensync/TMP/<timestamp>/` entries older than 2 days.
- `011.34` -- BAK/TMP cleanup determines the age of each cleanup candidate from the `<timestamp>` component of its directory name.
- `011.35` -- In `--dry-run`, KitchenSync skips BAK/TMP cleanup on peers.
- `011.36` -- KitchenSync does not purge `.kitchensync/SWAP/` directories by age.
- `011.37` -- During traversal, KitchenSync does not delete an existing user-entry SWAP directory when recovery for that swap directory fails.
- `011.38` -- If displacement to BAK fails, KitchenSync logs an error.
- `011.39` -- If displacement to BAK fails, KitchenSync skips that displacement.
- `011.40` -- If displacement to BAK fails, the entry remains in place.

## Notes
This category owns non-copy archive and cleanup behavior. The actual copy replacement sequence while a transfer is active belongs to `010_file-transfer-safety`. Snapshot database SWAP recovery and upload staging belong to `006_snapshot-lifecycle`; reusable timestamp string format rules belong to `016_snapshot-paths-and-timestamps`.
