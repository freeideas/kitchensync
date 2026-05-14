# Snapshot DB

## Purpose
Maintain a per-peer snapshot database: schema creation, path identifiers, timestamp generation, snapshot row lookup and mutation, tombstone handling, retention purge, and descendant cascade deletion.

## Public API
Data shapes:

- `SnapshotDatabase`: an opened SQLite database file containing the `snapshot` table
- `RelativePath`: path relative to the peer root, using forward slashes, with no leading slash and no trailing slash
- `PathId`: xxHash64 seed 0 of a `RelativePath`, base62-encoded to 11 zero-padded characters
- `ParentId`: `PathId` of the parent `RelativePath`, or the `PathId` of `/` for root entries
- `EntryType`: `file` or `directory`
- `SnapshotRow`: `id`, `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, `deleted_time`
- `Timestamp`: UTC timestamp in `YYYY-MM-DD_HH-mm-ss_ffffffZ`
- `RetentionCutoff`: `Timestamp` before which rows are expired

Operations:

- `open_or_create(database_path) -> SnapshotDatabase`
- `initialize(database)`
- `path_id(relative_path) -> PathId`
- `parent_id(relative_path) -> ParentId`
- `next_timestamp() -> Timestamp`
- `lookup(database, relative_path) -> SnapshotRow?`
- `upsert_confirmed_present(database, relative_path, entry_type, mod_time, byte_size, seen_at)`
- `mark_confirmed_absent(database, relative_path)`
- `record_push_decision(database, relative_path, entry_type, mod_time, byte_size)`
- `record_copy_completed(database, relative_path, seen_at)`
- `record_directory_created(database, relative_path, seen_at)`
- `record_decided_delete(database, relative_path)`
- `purge_expired(database, retention_cutoff)`
- `close(database)`

## Behavior
`initialize` creates exactly one table named `snapshot`, singular and lowercase. It has these columns: `id`, `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, and `deleted_time`. It creates indexes on `parent_id`, `last_seen`, and `deleted_time`. SQLite WAL mode is enabled and foreign keys are enabled.

`path_id` hashes normalized relative paths with xxHash64 seed 0 and base62-encodes the 64-bit value to 11 zero-padded characters. Directory and file paths hash identically; `byte_size = -1` distinguishes directories. The peer root itself is not represented by a row. Root children use the hash of `/` as `parent_id`.

`next_timestamp` returns UTC microsecond timestamps in `YYYY-MM-DD_HH-mm-ss_ffffffZ`. Within one process, every returned timestamp is strictly greater than every timestamp previously returned by that generator.

`lookup` returns the row for one relative path, or no row.

`upsert_confirmed_present` inserts or updates a row with the supplied `mod_time`, `byte_size`, `last_seen = seen_at`, and `deleted_time = NULL`. Directory rows use `byte_size = -1`.

`mark_confirmed_absent` is idempotent. If a row exists and `deleted_time` is NULL, it sets `deleted_time` to the row's current `last_seen` and leaves `last_seen` unchanged. If the row is already a tombstone or no row exists, it makes no change.

`record_push_decision` inserts or updates a row with the winning `mod_time`, `byte_size`, and `deleted_time = NULL`. It does not update `last_seen`; inserted rows have `last_seen = NULL`.

`record_copy_completed` sets `last_seen = seen_at` on the destination row after a completed file copy.

`record_directory_created` sets `last_seen = seen_at` on the destination row after directory creation succeeds.

`record_decided_delete` sets `deleted_time` on the target row to that row's current `last_seen`, then applies the same `deleted_time` to all descendant rows whose `deleted_time` is NULL by walking `parent_id` links from the deleted row's `id`.

`purge_expired` deletes tombstone rows whose `deleted_time` is older than `retention_cutoff`. It also deletes rows whose `deleted_time` is NULL and whose `last_seen` is older than `retention_cutoff` or NULL.

## Errors
Invalid relative paths return `invalid_path`.

Invalid timestamps return `invalid_timestamp`.

Invalid entry types or directory rows without `byte_size = -1` return `invalid_entry`.

Schema mismatches return `invalid_schema`.

SQLite open, read, write, transaction, or close failures return `sqlite_error`.

Hashing or base62 encoding failures return `path_id_error`.

## Anchoring
`SnapshotDatabase`, the `snapshot` table name, columns, indexes, WAL mode, and foreign keys are anchored in `database.md` "Schema".

`RelativePath`, `PathId`, `ParentId`, xxHash64 seed 0, base62 encoding, root sentinel `/`, and peer-root exclusion are anchored in `database.md` "Path Hashing".

`Timestamp`, timestamp format, UTC microsecond precision, and monotonic process behavior are anchored in `database.md` "Timestamps".

`SnapshotRow`, `last_seen`, `deleted_time`, tombstones, idempotent absence marking, and tombstone retention are anchored in `database.md` "Schema" and "Tombstones".

`upsert_confirmed_present`, `mark_confirmed_absent`, `record_push_decision`, `record_copy_completed`, `record_directory_created`, and `record_decided_delete` are anchored in `multi-tree-sync.md` "Snapshot Updates".

`purge_expired` and stale non-tombstone cleanup are anchored in `multi-tree-sync.md` "Orphaned Snapshot Rows" and `sync.md` "Run".

The descendant cascade behavior is anchored in `multi-tree-sync.md` "Snapshot Updates".

SQLite is anchored by the SQLite database format and SQL behavior. xxHash64 is anchored by the xxHash64 algorithm.
