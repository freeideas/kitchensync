# 021_staging-and-displacement: BAK/TMP staging, displacement, and cleanup

## Behavior
This concern derives from `specs/sync.md` sections "Displace to BAK", "TMP
Staging", "BAK Directory", and the displacement-failure error row of "Errors";
plus `specs/multi-tree-sync.md` section "BAK/TMP Cleanup During Traversal".

It covers the inline displacement operation: before renaming, create
`<parent>/.kitchensync/BAK/<timestamp>/` and any missing parents, then rename the
entry (a directory moves as a single subtree rename) into that BAK directory; a
displacement failure is logged and skipped, leaving the entry in place. It
covers the BAK directory's role (recoverable displaced entries, co-located in
`.kitchensync/` at each directory level rather than aggregated at the root) and
TMP staging's role (temporary metadata and cleanup work under `.kitchensync/`,
UUID per transfer). It covers age-based cleanup during traversal: at each
directory level, inspect each peer's `.kitchensync/` directly (a metadata
operation not subject to the built-in exclude), purge `BAK/<timestamp>/` entries
older than `--keep-bak-days` and `TMP/<timestamp>/` entries older than
`--keep-tmp-days` based on the timestamp in the name, and never purge SWAP by
age.

The SWAP staging that archives replaced files into BAK is `019_swap-replacement`.
The timestamp string format embedded in BAK/ and TMP/ names is `015_timestamps`.
The `X` progress line emitted for a displacement is `023_logging`.

## $REQ_IDs

- `021.1` -- Before renaming an entry for displacement, KitchenSync creates the directory `<parent>/.kitchensync/BAK/<timestamp>/`, including any missing parent directories.
- `021.2` -- Displacing the entry at `<parent>/<basename>` renames it to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `021.3` -- Displacing a directory moves it as a single rename, preserving its entire subtree.
- `021.4` -- The BAK/ directory for a displacement is created under `.kitchensync/` at the parent directory of the displaced entry, not aggregated at the sync root.
- `021.5` -- When the rename of a displaced entry into BAK/ fails, KitchenSync logs an error-level diagnostic.
- `021.6` -- When the rename of a displaced entry into BAK/ fails, the entry remains in place at its original path.
- `021.7` -- TMP staging directories are created under `.kitchensync/`.
- `021.8` -- Distinct transfers use distinct TMP staging directories so concurrent transfers do not collide.
- `021.9` -- In a normal run, after processing the union of entry names at a directory level, KitchenSync inspects each peer's `.kitchensync/` directory at that path.
- `021.10` -- KitchenSync purges BAK/ and TMP/ entries under `.kitchensync/` even though `.kitchensync/` is removed from synced listings by the built-in exclude.
- `021.11` -- Cleanup removes each `.kitchensync/BAK/<timestamp>/` entry whose timestamp is older than `--keep-bak-days` days.
- `021.12` -- Cleanup removes each `.kitchensync/TMP/<timestamp>/` entry whose timestamp is older than `--keep-tmp-days` days.
- `021.13` -- Cleanup determines each entry's age from the `<timestamp>` component of its directory name.
- `021.14` -- Cleanup leaves each `.kitchensync/BAK/<timestamp>/` entry whose timestamp is not older than `--keep-bak-days` days in place.
- `021.15` -- Cleanup leaves each `.kitchensync/TMP/<timestamp>/` entry whose timestamp is not older than `--keep-tmp-days` days in place.
- `021.16` -- Cleanup never removes `.kitchensync/SWAP/` entries based on age.
- `021.17` -- With no `--keep-bak-days` flag, cleanup uses a 90-day BAK retention limit.
- `021.18` -- With no `--keep-tmp-days` flag, cleanup uses a 2-day TMP retention limit.
- `021.19` -- In `--dry-run`, BAK/TMP cleanup on peers is skipped.

## Notes

The default values for `--keep-bak-days` (90) and `--keep-tmp-days` (2) are
stated in the named "BAK Directory" and "TMP Staging" sections and are kept here
as observable cleanup-retention behavior; a CLI-options category may also assert
flag parsing for the same flags without conflict.
