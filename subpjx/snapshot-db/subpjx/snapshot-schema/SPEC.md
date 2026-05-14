# Snapshot Schema

## Purpose
Open a SQLite snapshot database and initialize the required snapshot table, indexes, and connection settings.

## Public API
Data shapes:

- `SnapshotDatabase`: an opened SQLite database file containing the `snapshot` table

Operations:

- `open_or_create(database_path) -> SnapshotDatabase`
- `initialize(database)`
- `close(database)`

## Behavior
`open_or_create` opens the SQLite database file at `database_path`, creating it if it does not exist.

`initialize` creates exactly one table named `snapshot`, singular and lowercase. It has these columns: `id`, `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, and `deleted_time`. It creates indexes on `parent_id`, `last_seen`, and `deleted_time`. SQLite WAL mode is enabled and foreign keys are enabled.

`initialize` accepts an existing database only when the existing schema matches the required `snapshot` table and indexes.

`close` closes the opened SQLite database file.

## Errors
Schema mismatches return `invalid_schema`.

SQLite open, schema creation, index creation, pragma, transaction, or close failures return `sqlite_error`.

## Anchoring
`SnapshotDatabase`, the `snapshot` table name, columns, indexes, WAL mode, and foreign keys are anchored in `database.md` "Schema".

`open_or_create`, `initialize`, and `close` are anchored in the snapshot database operations.

SQLite is anchored by the SQLite database format and SQL behavior.
