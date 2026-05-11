# 04_bak-tmp-cleanup: BAK and TMP retention cleanup

## Behavior

Displaced files in `.kitchensync/BAK/` and stale staging in `.kitchensync/TMP/` are retained for a configurable number of days, then deleted. Cleanup happens during the combined-tree walk by inspecting the `.kitchensync/` directory at each level. Derived from `specs/multi-tree-sync.md` §"BAK/TMP Cleanup During Traversal" and `specs/sync.md` §"TMP Staging" / §"BAK Directory".

## $REQ_IDs
- `04.1` — Displaced files in `<parent>/.kitchensync/BAK/<timestamp>/` older than `--bd` days (default 90) are deleted during a sync run.
- `04.2` — Stale TMP staging in `<parent>/.kitchensync/TMP/<timestamp>/` older than `--xd` days (default 2) is deleted during a sync run.
- `04.3` — The age of a BAK/TMP entry is determined from its `<timestamp>` directory-name component (in the format `YYYY-MM-DD_HH-mm-ss_ffffffZ`).
