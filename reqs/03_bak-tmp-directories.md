# 03_bak-tmp-directories: BAK/ recovery and TMP/ atomic-swap staging

## Behavior

Files about to be overwritten or deleted are first displaced into a `BAK/` directory co-located with the affected file, so they remain recoverable for `--bd` days. File copies stage into a `TMP/` directory at the destination and atomically rename to the final path, so partial writes never appear at the target path. Both are housed under `.kitchensync/` at the parent directory level — never aggregated at the sync root. Derived from `./specs/sync.md` (`Operation Queue`, `File Copy`, `Displace to BAK`, `TMP Staging`, `BAK Directory`) and `./specs/multi-tree-sync.md` (inline-displacement paragraph).

## $REQ_IDs
- `03.51` — When a file at `<dir>/<name>` is overwritten by sync, the previous version is moved to `<dir>/.kitchensync/BAK/<timestamp>/<name>` before the new one appears.
- `03.52` — When a file at `<dir>/<name>` is removed by sync (deletion propagation), it is moved to `<dir>/.kitchensync/BAK/<timestamp>/<name>` rather than `delete`d.
- `03.53` — BAK/ directories are created at each affected directory level, not aggregated at the sync root.
- `03.54` — Each transferred file first appears under `<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>` and is then renamed to its final path.
- `03.55` — The final-path file resulting from a copy never contains partial content from a failed transfer (TMP staging is cleaned up on transfer failure before the final rename).
- `03.56` — A directory displaced to BAK/ is moved by a single rename and arrives in BAK/ with its entire subtree intact.
- `03.57` — `BAK/<timestamp>/` and `TMP/<timestamp>/` directory names use the `YYYY-MM-DD_HH-mm-ss_ffffffZ` format.
- `03.58` — Files placed in BAK/ remain readable from BAK/ for at least `--bd` days after placement (until cleanup).

## Notes

Retention/cleanup of BAK and TMP entries is covered in `04_cleanup`.
