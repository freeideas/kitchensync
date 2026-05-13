# 03_tmp-bak-staging: TMP staging and BAK displacement

## Behavior

Every file copy stages content into a `.kitchensync/TMP/<timestamp>/<uuid>/` directory near the destination and then atomically renames it into place; any pre-existing destination is first displaced to a `.kitchensync/BAK/<timestamp>/` directory in the same parent. Displacements are colocated with the displaced entry, not aggregated at the sync root. Derived from `sync.md` §"File Copy" / §"Displace to BAK" / §"TMP Staging" / §"BAK Directory".

## $REQ_IDs

- `03.28` — A copy first writes its content to `<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>` on the destination peer.
- `03.29` — If the destination already has a file at the target path, that file is renamed to `<file-parent>/.kitchensync/BAK/<timestamp>/<basename>` before the new file is renamed into place.
- `03.30` — The final placement of the new file is a same-filesystem rename from TMP into the target path (atomic).
- `03.31` — After a copy completes, the destination file's mod_time is set to the winning mod_time from the decision (not re-read from the source).
- `03.32` — BAK and TMP directories are created at the parent directory of each affected entry, not aggregated at the sync root.
- `03.33` — Both TMP and BAK timestamp directories use the format `YYYY-MM-DD_HH-mm-ss_ffffffZ` (UTC, microsecond precision).
- `03.34` — A displaced directory is moved to BAK/ as a single rename, preserving its entire subtree.
- `03.35` — On transfer failure, the TMP staging file (or directory) for that transfer is deleted; no partially-written file replaces the destination.
- `03.89` — After a successful file copy, the empty per-transfer TMP `<timestamp>/<uuid>/` directory left by the rename is removed (not left for the `--xd` age-based sweep).

## Notes

Retention windows (`--xd` for TMP, `--bd` for BAK) are in `04_retention.md`.
