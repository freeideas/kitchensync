# 005_snapshot-storage: Snapshot database format

## Behavior
This concern derives from `specs/database.md` sections "Database", "Schema", and "Tombstones". It covers the peer snapshot database location as peer state, SQLite storage expectations, the exact `snapshot` table schema and indexes, row field meanings, tombstone representation, and the rule that the sync root itself is not tracked as a row.

## $REQ_IDs
- `005.1` -- Each peer stores its snapshot database at `{peer-root}/.kitchensync/snapshot.db`.
- `005.2` -- The peer state includes `.kitchensync/snapshot.db` as the peer's snapshot database file.
- `005.3` -- The peer state excludes SQLite sidecar files for `.kitchensync/snapshot.db`.
- `005.4` -- The snapshot database is a SQLite database.
- `005.5` -- The snapshot database uses rollback-journal mode.
- `005.6` -- The snapshot database schema contains exactly one non-internal table.
- `005.7` -- The sole non-internal table in the snapshot database is named `snapshot`.
- `005.8` -- The snapshot database schema contains no SQL views.
- `005.9` -- The `snapshot` table has exactly the columns `id`, `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, and `deleted_time`.
- `005.10` -- The `snapshot` table represents each tracked path with one row.
- `005.11` -- The sync root directory itself has no row in the `snapshot` table.
- `005.12` -- The `snapshot` table has an `id` column with SQL type `TEXT`.
- `005.13` -- The `snapshot.id` column is the table primary key.
- `005.14` -- Each `snapshot.id` value stores the xxHash64 of the entry's full relative path as an 11-character base62 string.
- `005.15` -- The `snapshot` table has a `parent_id` column with SQL type `TEXT`.
- `005.16` -- Each `snapshot.parent_id` value stores the xxHash64 of the entry's parent directory relative path as a base62 string.
- `005.17` -- Snapshot rows for root entries store the hash of `/` as the `parent_id` sentinel.
- `005.18` -- The `snapshot` table has a `basename` column with SQL type `TEXT`.
- `005.19` -- The `snapshot.basename` column is not nullable.
- `005.20` -- Each `snapshot.basename` value stores the entry's final path component.
- `005.21` -- The `snapshot` table has a `mod_time` column with SQL type `TEXT`.
- `005.22` -- The `snapshot.mod_time` column is not nullable.
- `005.23` -- Each `snapshot.mod_time` value uses the `YYYY-MM-DD_HH-mm-ss_ffffffZ` timestamp format.
- `005.24` -- Each `snapshot.mod_time` value stores the entry's modification time as last observed on that peer.
- `005.25` -- The `snapshot` table has a `byte_size` column with SQL type `INTEGER`.
- `005.26` -- The `snapshot.byte_size` column is not nullable.
- `005.27` -- Snapshot rows for files store the file size in bytes in `byte_size`.
- `005.28` -- Snapshot rows for directories store `-1` in `byte_size`.
- `005.29` -- The `snapshot` table has a `last_seen` column with SQL type `TEXT`.
- `005.30` -- Each non-NULL `snapshot.last_seen` value uses the `YYYY-MM-DD_HH-mm-ss_ffffffZ` timestamp format.
- `005.31` -- A non-NULL `snapshot.last_seen` value records when the entry was confirmed present by listing or completed copy.
- `005.32` -- A NULL `snapshot.last_seen` value records that a copy has been decided but has not completed.
- `005.33` -- The `snapshot` table has a `deleted_time` column with SQL type `TEXT`.
- `005.34` -- Each non-NULL `snapshot.deleted_time` value uses the `YYYY-MM-DD_HH-mm-ss_ffffffZ` timestamp format.
- `005.35` -- A NULL `snapshot.deleted_time` value represents an entry that exists.
- `005.36` -- A non-NULL `snapshot.deleted_time` value represents a tombstone.
- `005.37` -- When an entry is confirmed absent from a peer and its existing snapshot row has `deleted_time = NULL`, the row is retained.
- `005.38` -- When an existing snapshot row is retained after absence confirmation, `deleted_time` is set to that row's current `last_seen` value.
- `005.39` -- Reconfirming absence for a snapshot row whose `deleted_time` is already set leaves the existing tombstone unchanged.
- `005.40` -- The `snapshot` table has an index on `parent_id`.
- `005.41` -- The `snapshot` table has an index on `last_seen`.
- `005.42` -- The `snapshot` table has an index on `deleted_time`.

## Notes
This category owns the static database file and table format. Snapshot path normalization, hash seed and padding rules, and timestamp monotonicity belong to `016_snapshot-paths-and-timestamps`; snapshot download/upload behavior belongs to `006_snapshot-lifecycle`; row mutation timing during sync, including tombstone purging, belongs to `009_snapshot-updates`.
