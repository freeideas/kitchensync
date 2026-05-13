# 04_retention: TMP, BAK, and tombstone retention

## Behavior

Stale staging files, displaced files, and snapshot tombstones are eventually purged. Retention windows are controlled by `--xd` (TMP), `--bd` (BAK), and `--td` (tombstones / stale rows). TMP/BAK cleanup happens during the multi-tree walk at each level's `.kitchensync/`; tombstone purge happens at startup. Derived from `sync.md` §Run / §"TMP Staging" / §"BAK Directory", `multi-tree-sync.md` §"BAK/TMP Cleanup During Traversal" / §"Orphaned Snapshot Rows", and `database.md` §Tombstones.

## $REQ_IDs

- `04.1` — At startup, snapshot rows with `deleted_time IS NOT NULL` and `deleted_time` older than `--td` days are deleted.
- `04.2` — At startup, snapshot rows with `deleted_time IS NULL` and `last_seen` older than `--td` days (or `last_seen IS NULL`) are deleted.
- `04.3` — During the multi-tree walk at each directory level, entries inside that level's `.kitchensync/BAK/` whose `<timestamp>` directory is older than `--bd` days are removed.
- `04.4` — During the multi-tree walk at each directory level, entries inside that level's `.kitchensync/TMP/` whose `<timestamp>` directory is older than `--xd` days are removed.

## Notes

The age comparison uses the `<timestamp>` component of the TMP/BAK subdirectory name, which is formatted per `database.md` §Timestamps. Default values for `--xd`, `--bd`, and `--td` are in `01_cli-grammar.md`.
