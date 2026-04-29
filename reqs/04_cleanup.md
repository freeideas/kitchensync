# 04_cleanup: Retention purges for BAK/, TMP/, and snapshot tombstones

## Behavior

Stale staging files, displaced files, and snapshot tombstones are aged out. BAK/ entries older than `--bd` days are removed; TMP/ entries older than `--xd` days are removed; snapshot rows older than `--td` days are removed at startup. BAK/ and TMP/ purges piggy-back on the multi-tree traversal at each `.kitchensync/` directory encountered. Derived from `./specs/multi-tree-sync.md` (`BAK/TMP Cleanup During Traversal`, `Orphaned Snapshot Rows`), `./specs/sync.md` (`Run` step 1, `BAK Directory`, `TMP Staging`), and `./specs/database.md` (`Tombstones`).

## $REQ_IDs
- `04.11` — A `.kitchensync/BAK/<timestamp>/` subdirectory whose timestamp is older than `--bd` days is removed during a run.
- `04.12` — A `.kitchensync/BAK/<timestamp>/` subdirectory whose timestamp is younger than `--bd` days is left in place during a run.
- `04.13` — A `.kitchensync/TMP/<timestamp>/` subdirectory whose timestamp is older than `--xd` days is removed during a run.
- `04.14` — A `.kitchensync/TMP/<timestamp>/` subdirectory whose timestamp is younger than `--xd` days is left in place during a run.
- `04.15` — Snapshot rows with `deleted_time` older than `--td` days are deleted at startup, before traversal begins.
- `04.16` — Snapshot rows with `deleted_time IS NULL` and `last_seen` older than `--td` days (or NULL) are deleted at the same startup purge.
- `04.17` — BAK/ and TMP/ purges occur at every `.kitchensync/` directory the traversal encounters at any tree level, not only at the sync root.
- `04.18` — The age of a `BAK/<timestamp>/` or `TMP/<timestamp>/` entry is determined by the timestamp embedded in its directory name, not by filesystem mtime.
