# 018_dry-run: Read-only sync planning mode

## Behavior
This concern derives from `specs/sync.md` sections "Global Options", "Startup",
"Run", and "Dry Run", `specs/database.md` opening section, and
`specs/multi-tree-sync.md` sections "SWAP Recovery During Traversal" and
"BAK/TMP Cleanup During Traversal". It covers the observable behavior of
`--dry-run`: realistic connection, listing, local temporary snapshot updates,
copy queue exercise, source reads, progress output, dry-run marker text, and
the prohibition on creating, modifying, renaming, deleting, displacing,
cleaning, or uploading through peer URLs.

## $REQ_IDs
- `018.1` -- `--dry-run` makes KitchenSync connect to peer URLs during startup.
- `018.2` -- In `--dry-run`, KitchenSync does not create a missing peer root directory or a missing peer root parent directory.
- `018.3` -- In `--dry-run`, KitchenSync treats a peer URL whose root path does not already exist as unreachable for that run.
- `018.4` -- In `--dry-run`, KitchenSync skips peer-side `.kitchensync/SWAP/snapshot.db/` recovery before snapshot download.
- `018.5` -- In `--dry-run`, KitchenSync downloads an existing peer `.kitchensync/snapshot.db` file as the live file currently present on that peer.
- `018.6` -- In `--dry-run`, KitchenSync creates a new empty local temporary snapshot database for a reachable peer that has no `.kitchensync/snapshot.db`.
- `018.7` -- In `--dry-run`, KitchenSync lists peer directories for sync decisions.
- `018.8` -- In `--dry-run`, KitchenSync skips peer-side `.kitchensync/SWAP/` recovery during traversal.
- `018.9` -- In `--dry-run`, KitchenSync updates local temporary snapshot databases during traversal.
- `018.10` -- In `--dry-run`, KitchenSync exercises the copy queue for planned file copies.
- `018.11` -- In `--dry-run`, queued copy work acquires active-copy slots.
- `018.12` -- In `--dry-run`, queued copy work reads source files.
- `018.13` -- In `--dry-run`, queued copy work applies the `--retries-copy` total try limit.
- `018.14` -- In `--dry-run`, KitchenSync emits `C` progress lines for copy work in the same cases as a normal run.
- `018.15` -- In `--dry-run`, KitchenSync emits `X` progress lines for failed copy work in the same cases as a normal run.
- `018.16` -- In `--dry-run`, KitchenSync prints the phrase `dry run` to stdout at least once.
- `018.17` -- In `--dry-run`, KitchenSync creates no peer directories through a `file://` or `sftp://` peer URL.
- `018.18` -- In `--dry-run`, KitchenSync creates no peer files through a `file://` or `sftp://` peer URL.
- `018.19` -- In `--dry-run`, KitchenSync writes no destination file content through a `file://` or `sftp://` peer URL.
- `018.20` -- In `--dry-run`, KitchenSync renames no peer entries through a `file://` or `sftp://` peer URL.
- `018.21` -- In `--dry-run`, KitchenSync deletes no destination files through a `file://` or `sftp://` peer URL.
- `018.22` -- In `--dry-run`, KitchenSync displaces no destination entries to peer BAK storage.
- `018.23` -- In `--dry-run`, KitchenSync sets no modification times through a `file://` or `sftp://` peer URL.
- `018.24` -- In `--dry-run`, KitchenSync does not upload updated local temporary snapshot databases back to peers.
- `018.25` -- In `--dry-run`, KitchenSync skips peer-side BAK cleanup during traversal.
- `018.26` -- In `--dry-run`, KitchenSync skips peer-side TMP cleanup during traversal.

## Notes
This file owns dry-run deviations from normal behavior so other categories can
state normal-run behavior without duplicating every dry-run exception.
