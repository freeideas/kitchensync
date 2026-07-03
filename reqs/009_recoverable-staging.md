# 009_recoverable-staging: Recoverable staging and cleanup

## Behavior
This concern derives from `specs/sync.md` sections "Rename Compatibility",
"File Copy", "Displace to BAK", "TMP Staging", "SWAP Directory", "BAK
Directory", and "Errors", `specs/multi-tree-sync.md` sections "SWAP Recovery
During Traversal", "BAK/TMP Cleanup During Traversal", "Directory Decisions",
and "Type Conflicts", `specs/database.md` sections "Database" and "Snapshot
SWAP recovery", and `specs/SCENARIOS.md` scenarios S-05, S-06, and S-09. It
covers the observable use of SWAP `new` and `old` paths for user-file
replacement, recovery of incomplete user-file swaps, same-filesystem
displacement of files and directories to BAK, BAK and TMP timestamp directory
placement, cleanup retention rules, failure behavior around staging, and the
requirement not to depend on rename-over-existing behavior.

## $REQ_IDs

- `009.1` -- Replacing an existing user file succeeds on a transport that rejects renaming a source path over an existing destination path.
- `009.2` -- Replacing an existing user file writes the replacement bytes to `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new` before changing the live destination file.
- `009.3` -- Replacing an existing user file moves the previous live file to `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old` before moving `new` to the live destination path.
- `009.4` -- After a successful existing-file replacement, the live destination file contains the replacement bytes.
- `009.5` -- After a successful existing-file replacement, the live destination file has the winning modification time from the sync decision.
- `009.6` -- After a successful existing-file replacement, the replaced file is recoverable at `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `009.7` -- After a successful existing-file replacement, the empty SWAP directory for that basename is removed.
- `009.8` -- If an existing-file replacement fails while moving the live destination to SWAP `old`, the original live destination file remains in place.
- `009.9` -- If a file transfer fails before SWAP `old` exists, KitchenSync removes that transfer's SWAP `new` staging when removal is possible.
- `009.10` -- If an existing-file replacement fails after SWAP `old` exists, KitchenSync leaves the SWAP state for a later recovery run.
- `009.11` -- If archiving SWAP `old` to BAK fails after the replacement is live, KitchenSync leaves SWAP `old` for a later recovery run.
- `009.12` -- A normal run recovers an existing user-file SWAP directory before listing that directory's live user entries for sync decisions.
- `009.13` -- When SWAP `old` exists and the live target exists, user-file SWAP recovery deletes SWAP `new` if present.
- `009.14` -- When SWAP `old` exists and the live target exists, user-file SWAP recovery moves SWAP `old` to BAK.
- `009.15` -- When SWAP `old` exists, SWAP `new` exists, and the live target is missing, user-file SWAP recovery moves SWAP `new` to the live target path.
- `009.16` -- When SWAP `old` exists, SWAP `new` exists, and the live target is missing, user-file SWAP recovery moves SWAP `old` to BAK.
- `009.17` -- When SWAP `old` exists, SWAP `new` is missing, and the live target is missing, user-file SWAP recovery moves SWAP `old` back to the live target path.
- `009.18` -- When SWAP `old` is missing, SWAP `new` exists, and the live target exists, user-file SWAP recovery deletes SWAP `new`.
- `009.19` -- When SWAP `old` is missing, SWAP `new` exists, and the live target is missing, user-file SWAP recovery moves SWAP `new` to the live target path.
- `009.20` -- Successful user-file SWAP recovery removes the recovered entry's empty SWAP directory.
- `009.21` -- If user-file SWAP recovery fails for a peer at a directory, KitchenSync skips sync decisions for that peer's current directory subtree.
- `009.22` -- A SWAP directory for a user path is placed at `<target-parent>/.kitchensync/SWAP/<encoded-basename>/`.
- `009.23` -- SWAP uses `new` and `old` as the only live staging path names for a user-file replacement.
- `009.24` -- The SWAP `<encoded-basename>` path segment percent-encodes the target basename when the basename cannot be used directly as one path segment on every supported transport.
- `009.25` -- Before starting a replacement for a user path, KitchenSync recovers or fails any existing SWAP directory for that path's basename.
- `009.26` -- A displacement creates `<parent>/.kitchensync/BAK/<timestamp>/` and any missing parents before moving the displaced entry.
- `009.27` -- A displaced file is moved to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `009.28` -- A displaced directory is moved to `<parent>/.kitchensync/BAK/<timestamp>/<basename>` as one directory tree.
- `009.29` -- If a displacement to BAK fails, the original live path remains in place.
- `009.30` -- When a deletion decision wins for a file, KitchenSync displaces each remaining live copy of that file to BAK.
- `009.31` -- When a deletion decision wins for a directory, KitchenSync displaces each remaining live copy of that directory to BAK.
- `009.32` -- When all contributing peers have no live file and no snapshot row for a path, KitchenSync displaces each subordinate live file at that path to BAK.
- `009.33` -- When no contributing peer has a directory live or in its snapshot rows for a path, KitchenSync displaces each subordinate live directory at that path to BAK.
- `009.34` -- When a canon file conflicts with a directory at the same path, KitchenSync displaces the directory to BAK before placing the canon file at that path.
- `009.35` -- When a canon directory conflicts with a file at the same path, KitchenSync displaces the file to BAK before creating or syncing the canon directory at that path.
- `009.36` -- Without a canon peer, when a contributing file conflicts with a contributing directory at the same path, KitchenSync displaces the contributing directory to BAK.
- `009.37` -- After the contributing peers decide a type-conflict outcome, KitchenSync displaces a subordinate peer's wrong-type path to BAK.
- `009.38` -- BAK timestamp directory names use the `YYYY-MM-DD_HH-mm-ss_ffffffZ` format.
- `009.39` -- TMP staging for temporary metadata and cleanup work is placed under `.kitchensync/TMP/<timestamp>/`.
- `009.40` -- TMP timestamp directory names use the `YYYY-MM-DD_HH-mm-ss_ffffffZ` format.
- `009.41` -- Concurrent TMP staging paths do not collide across transfers.
- `009.42` -- In a normal run, BAK and TMP cleanup checks the `.kitchensync/` directory at each visited directory level.
- `009.43` -- In a normal run, KitchenSync removes `.kitchensync/BAK/<timestamp>/` directories older than `--keep-bak-days`.
- `009.44` -- With no `--keep-bak-days` option, KitchenSync removes `.kitchensync/BAK/<timestamp>/` directories older than 90 days.
- `009.45` -- BAK cleanup leaves `.kitchensync/BAK/<timestamp>/` directories that are not older than `--keep-bak-days`.
- `009.46` -- In a normal run, KitchenSync removes `.kitchensync/TMP/<timestamp>/` directories older than `--keep-tmp-days`.
- `009.47` -- With no `--keep-tmp-days` option, KitchenSync removes `.kitchensync/TMP/<timestamp>/` directories older than 2 days.
- `009.48` -- TMP cleanup leaves `.kitchensync/TMP/<timestamp>/` directories that are not older than `--keep-tmp-days`.
- `009.49` -- BAK and TMP cleanup determines age from each directory's `<timestamp>` path segment.
- `009.50` -- BAK and TMP cleanup does not remove `.kitchensync/SWAP/` directories by age.
- `009.51` -- If TMP staging cannot be created or written for a transfer, KitchenSync handles the operation as a transfer failure.
- `009.52` -- If SWAP staging cannot be created or written for a transfer, KitchenSync handles the operation as a transfer failure.

## Notes
Snapshot database replacement uses the same SWAP pattern, but the database file
lifecycle is owned by `004_snapshot-database-lifecycle`.
