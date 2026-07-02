# 017_staging-recovery-and-cleanup: SWAP, BAK, and TMP peer state

## Behavior
This concern derives from `specs/sync.md` sections "Rename Compatibility",
"Displace to BAK", "TMP Staging", "SWAP Directory", and "BAK Directory",
`specs/multi-tree-sync.md` sections "SWAP Recovery During Traversal" and
"BAK/TMP Cleanup During Traversal", and `specs/database.md` opening section. It
covers peer-side SWAP path layout, encoded SWAP basenames, user-data replacement
recovery, snapshot replacement recovery, inline displacement to nearby BAK
directories, BAK and TMP path layout, age-based BAK/TMP cleanup, and the rule
that SWAP directories are recovered rather than purged by age.

## $REQ_IDs
- `017.1` -- KitchenSync replaces an existing user file on transports whose `rename(src, dst)` rejects an existing destination.
- `017.2` -- For replacement of user path `<parent>/<basename>`, the SWAP `new` path is `<parent>/.kitchensync/SWAP/<encoded-basename>/new`.
- `017.3` -- For replacement of user path `<parent>/<basename>`, the SWAP `old` path is `<parent>/.kitchensync/SWAP/<encoded-basename>/old`.
- `017.4` -- `<encoded-basename>` percent-encodes the basename when needed so the encoded value is one path segment on every supported transport.
- `017.5` -- Before starting replacement of a user path, KitchenSync recovers any existing SWAP directory for that path's encoded basename or treats recovery as failed.
- `017.6` -- During normal traversal, KitchenSync checks each peer for `.kitchensync/SWAP/` at a directory level before listing that directory's live entries for sync decisions.
- `017.7` -- During normal traversal, each direct child of `.kitchensync/SWAP/` is recovered as SWAP state for the corresponding user entry in the same parent directory.
- `017.8` -- If SWAP `old` and the target both exist during user-data SWAP recovery, KitchenSync leaves the target in place.
- `017.9` -- If SWAP `old` and the target both exist during user-data SWAP recovery, KitchenSync moves SWAP `old` to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `017.10` -- If SWAP `old` and the target both exist during user-data SWAP recovery, KitchenSync removes the empty SWAP directory.
- `017.11` -- If SWAP `old` and SWAP `new` both exist while the target is missing during user-data SWAP recovery, KitchenSync renames SWAP `new` to the target path.
- `017.12` -- If SWAP `old` and SWAP `new` both exist while the target is missing during user-data SWAP recovery, KitchenSync moves SWAP `old` to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `017.13` -- If SWAP `old` and SWAP `new` both exist while the target is missing during user-data SWAP recovery, KitchenSync removes the empty SWAP directory.
- `017.14` -- If SWAP `old` exists while SWAP `new` and the target are both missing during user-data SWAP recovery, KitchenSync renames SWAP `old` back to the target path.
- `017.15` -- If SWAP `old` exists while SWAP `new` and the target are both missing during user-data SWAP recovery, KitchenSync removes the empty SWAP directory.
- `017.16` -- If SWAP `new` and the target both exist while SWAP `old` is missing during user-data SWAP recovery, KitchenSync leaves the target in place.
- `017.17` -- If SWAP `new` and the target both exist while SWAP `old` is missing during user-data SWAP recovery, KitchenSync deletes SWAP `new`.
- `017.18` -- If SWAP `new` and the target both exist while SWAP `old` is missing during user-data SWAP recovery, KitchenSync removes the empty SWAP directory.
- `017.19` -- If SWAP `new` exists while SWAP `old` and the target are both missing during user-data SWAP recovery, KitchenSync renames SWAP `new` to the target path.
- `017.20` -- If SWAP `new` exists while SWAP `old` and the target are both missing during user-data SWAP recovery, KitchenSync removes the empty SWAP directory.
- `017.21` -- If user-data SWAP recovery fails for a directory on a peer, KitchenSync treats that peer's listing for the current directory as failed.
- `017.22` -- If user-data SWAP recovery fails for a directory on a peer, KitchenSync leaves that peer's snapshot rows for the current directory subtree unchanged.
- `017.23` -- Snapshot replacement uses `.kitchensync/SWAP/snapshot.db/new` as the snapshot SWAP `new` path.
- `017.24` -- Snapshot replacement uses `.kitchensync/SWAP/snapshot.db/old` as the snapshot SWAP `old` path.
- `017.25` -- During normal startup, KitchenSync recovers incomplete `.kitchensync/SWAP/snapshot.db/` state before deciding whether the peer has snapshot history.
- `017.26` -- If snapshot SWAP `old` and live `snapshot.db` both exist during snapshot SWAP recovery, KitchenSync leaves live `snapshot.db` in place.
- `017.27` -- If snapshot SWAP `old` and live `snapshot.db` both exist during snapshot SWAP recovery, KitchenSync deletes snapshot SWAP `old`.
- `017.28` -- If snapshot SWAP `old` and live `snapshot.db` both exist during snapshot SWAP recovery, KitchenSync deletes snapshot SWAP `new` when it is present.
- `017.29` -- If snapshot SWAP `old` and snapshot SWAP `new` both exist while live `snapshot.db` is missing during snapshot SWAP recovery, KitchenSync renames snapshot SWAP `new` to live `snapshot.db`.
- `017.30` -- If snapshot SWAP `old` and snapshot SWAP `new` both exist while live `snapshot.db` is missing during snapshot SWAP recovery, KitchenSync deletes snapshot SWAP `old`.
- `017.31` -- If snapshot SWAP `old` exists while snapshot SWAP `new` and live `snapshot.db` are both missing during snapshot SWAP recovery, KitchenSync renames snapshot SWAP `old` to live `snapshot.db`.
- `017.32` -- If snapshot SWAP `new` and live `snapshot.db` both exist while snapshot SWAP `old` is missing during snapshot SWAP recovery, KitchenSync leaves live `snapshot.db` in place.
- `017.33` -- If snapshot SWAP `new` and live `snapshot.db` both exist while snapshot SWAP `old` is missing during snapshot SWAP recovery, KitchenSync deletes snapshot SWAP `new`.
- `017.34` -- If snapshot SWAP `new` exists while snapshot SWAP `old` and live `snapshot.db` are both missing during snapshot SWAP recovery, KitchenSync renames snapshot SWAP `new` to live `snapshot.db`.
- `017.35` -- When KitchenSync displaces entry `<parent>/<basename>`, the displaced entry is moved to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `017.36` -- Before displacing an entry, KitchenSync creates `<parent>/.kitchensync/BAK/<timestamp>/` and its missing parents when they do not already exist.
- `017.37` -- BAK directories for displacements are created under the displaced entry's parent directory rather than aggregated at the sync root.
- `017.38` -- After KitchenSync displaces an entry, the displaced entry is absent from its original path.
- `017.39` -- When KitchenSync displaces a directory, the directory's subtree is preserved under the BAK destination.
- `017.40` -- TMP staging paths KitchenSync creates are under `.kitchensync/TMP/<timestamp>/`.
- `017.41` -- Each TMP staging path KitchenSync creates for transfer work includes a transfer UUID.
- `017.42` -- TMP staging does not replace a live user path.
- `017.43` -- During normal traversal, after processing the union of entry names at a directory level, KitchenSync checks that directory's `.kitchensync/BAK/` and `.kitchensync/TMP/` subdirectories for cleanup.
- `017.44` -- BAK/TMP cleanup checks `.kitchensync/` metadata directories even though `.kitchensync/` is excluded from sync decisions.
- `017.45` -- BAK/TMP cleanup determines each staging directory's age from the `<timestamp>` component of the staging directory path.
- `017.46` -- BAK/TMP cleanup removes `.kitchensync/BAK/<timestamp>/` directories older than `--keep-bak-days`.
- `017.47` -- BAK/TMP cleanup leaves `.kitchensync/BAK/<timestamp>/` directories that are not older than `--keep-bak-days`.
- `017.48` -- BAK/TMP cleanup removes `.kitchensync/TMP/<timestamp>/` directories older than `--keep-tmp-days`.
- `017.49` -- BAK/TMP cleanup leaves `.kitchensync/TMP/<timestamp>/` directories that are not older than `--keep-tmp-days`.
- `017.50` -- BAK/TMP cleanup does not purge `.kitchensync/SWAP/` directories by age.
- `017.51` -- SWAP directories left by interrupted work remain until SWAP recovery succeeds.

## Notes
This file covers peer-side staging state and recovery. Snapshot row effects of
successful displacement belong to `015_snapshot-row-updates-and-cleanup.md`.
Queued transfer sequencing belongs to `016_copy-queue-and-transfers.md`.
Dry-run write prohibitions belong to `018_dry-run.md`.
