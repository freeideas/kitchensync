# 007_snapshot-schema: Snapshot table shape and row meanings

## Behavior
This concern derives from `specs/database.md` sections "Schema" and
"Tombstones", and `plan/sqlite-snapshot.md`. It covers the exact SQLite
`snapshot` table name, columns, indexes, row-level field meanings, directory
byte size marker, tombstone representation, and the rule that only children of
the sync root are tracked.

## $REQ_IDs
- `007.1` -- A peer snapshot database contains exactly one SQLite table named `snapshot`.
- `007.2` -- A peer snapshot database contains no SQLite views.
- `007.3` -- The `snapshot` table contains exactly these columns: `id`, `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, and `deleted_time`.
- `007.4` -- The `snapshot.id` column has SQLite type `TEXT` and primary-key status.
- `007.5` -- The `snapshot.id` value is the 11-character base62 xxHash64 path ID for the row's full relative path.
- `007.6` -- The `snapshot.parent_id` column has SQLite type `TEXT`.
- `007.7` -- The `snapshot.parent_id` value is the 11-character base62 xxHash64 path ID for the row's parent directory relative path.
- `007.8` -- Rows for entries directly below the sync root use the path ID for `/` as `snapshot.parent_id`.
- `007.9` -- The `snapshot.basename` column has SQLite type `TEXT` and is `NOT NULL`.
- `007.10` -- The `snapshot.basename` value is the final path component of the row's path.
- `007.11` -- The `snapshot.mod_time` column has SQLite type `TEXT` and is `NOT NULL`.
- `007.12` -- The `snapshot.mod_time` value is the entry modification time last observed on that peer.
- `007.13` -- `snapshot.mod_time` values use `YYYY-MM-DD_HH-mm-ss_ffffffZ` format.
- `007.14` -- The `snapshot.byte_size` column has SQLite type `INTEGER` and is `NOT NULL`.
- `007.15` -- File rows store the file size in bytes as `snapshot.byte_size`.
- `007.16` -- Directory rows store `-1` as `snapshot.byte_size`.
- `007.17` -- The `snapshot.last_seen` column has SQLite type `TEXT` and allows NULL.
- `007.18` -- A non-NULL `snapshot.last_seen` value records when the entry was confirmed present by listing or completed copy.
- `007.19` -- A NULL `snapshot.last_seen` value represents a copy destination that has been decided but not completed.
- `007.20` -- Non-NULL `snapshot.last_seen` values use `YYYY-MM-DD_HH-mm-ss_ffffffZ` format.
- `007.21` -- The `snapshot.deleted_time` column has SQLite type `TEXT` and allows NULL.
- `007.22` -- A NULL `snapshot.deleted_time` value marks the row as not tombstoned.
- `007.23` -- A non-NULL `snapshot.deleted_time` value marks the row as a tombstone.
- `007.24` -- Non-NULL `snapshot.deleted_time` values use `YYYY-MM-DD_HH-mm-ss_ffffffZ` format.
- `007.25` -- Tombstone rows remain in the `snapshot` table.
- `007.26` -- A newly created tombstone stores the row's current `last_seen` value in `snapshot.deleted_time`.
- `007.27` -- The `snapshot` table has an index on `parent_id`.
- `007.28` -- The `snapshot` table has an index on `last_seen`.
- `007.29` -- The `snapshot` table has an index on `deleted_time`.
- `007.30` -- The `snapshot` table stores one row for each tracked path the peer has or has had.
- `007.31` -- Every snapshot row represents a path below the sync root.
- `007.32` -- The sync root directory itself has no row in `snapshot`.

## Notes
This file covers stored shape and field meaning. The algorithms that change
rows belong to `015_snapshot-row-updates-and-cleanup.md`.
