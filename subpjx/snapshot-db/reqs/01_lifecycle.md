# 01_lifecycle: Open and close a snapshot database file.

## Behavior
A snapshot database lives at a local filesystem path. Opening it returns a handle the caller uses for all subsequent operations; closing the handle flushes pending writes. A fresh open at a non-existent path creates the file with the snapshot schema in place; opening an existing file reuses the schema. Derived from `SPEC.md` §"Lifecycle" and `SPEC.md` §"Schema (informational)".

## $REQ_IDs
- `01.1` — Opening a snapshot database at a path that does not exist creates a new file at that path.
- `01.2` — A newly created snapshot database contains a `snapshot` table that accepts inserts using the documented columns (`id`, `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, `deleted_time`).
- `01.3` — Opening an existing snapshot database returns a handle that can read prior rows and write new rows.
- `01.4` — An open snapshot database uses SQLite WAL journal mode.
- `01.5` — An open snapshot database has SQLite foreign-key enforcement enabled.
- `01.6` — Closing a handle flushes pending writes so that re-opening the same path observes the prior changes.

## Notes
Schema column list, types, and NULL rules are specified by `database.md` §"Schema" (anchored by `SPEC.md`). Index presence (on `parent_id`, `last_seen`, `deleted_time`) is a performance mechanism and is not tested as a separate behavior.
