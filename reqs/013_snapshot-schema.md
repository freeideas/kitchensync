# 013_snapshot-schema: Snapshot database schema

## Behavior
This concern derives from `specs/database.md` section "Schema".

It covers the observable SQLite schema of a peer snapshot database: exactly one
table named `snapshot` (singular, lowercase, no synonym or view), one row per
path, and the columns with their types and meanings - `id` (text primary key),
`parent_id`, `basename` (not null), `mod_time` (not null; informational only for
directories), `byte_size` (not null; bytes for files, -1 for directories),
`last_seen` (text or NULL), and `deleted_time` (text or NULL). It covers the
indexes on `parent_id`, `last_seen`, and `deleted_time`.

How `id` and `parent_id` values are computed is `014_path-hashing`. The
timestamp string format used in `mod_time`, `last_seen`, and `deleted_time` is
`015_timestamps`. When and how rows are written and tombstoned is
`017_snapshot-updates`. Where the database file lives and how it is moved between
peers is `016_snapshot-storage`.

## $REQ_IDs
- `013.1` -- A peer snapshot database contains exactly one table.
- `013.2` -- The single table is named `snapshot`.
- `013.3` -- A peer snapshot database contains no view.
- `013.4` -- The `snapshot` table has a column named `id` of type TEXT.
- `013.5` -- The `id` column is the primary key of the `snapshot` table.
- `013.6` -- The `snapshot` table has a column named `parent_id` of type TEXT.
- `013.7` -- The `snapshot` table has a column named `basename` of type TEXT.
- `013.8` -- The `basename` column is not null.
- `013.9` -- The `snapshot` table has a column named `mod_time` of type TEXT.
- `013.10` -- The `mod_time` column is not null.
- `013.11` -- The `snapshot` table has a column named `byte_size` of type INTEGER.
- `013.12` -- The `byte_size` column is not null.
- `013.13` -- A `snapshot` row for a file stores the file's size in bytes in `byte_size`.
- `013.14` -- A `snapshot` row for a directory stores `-1` in `byte_size`.
- `013.15` -- The `snapshot` table has a column named `last_seen` of type TEXT that permits NULL.
- `013.16` -- The `snapshot` table has a column named `deleted_time` of type TEXT that permits NULL.
- `013.17` -- The `snapshot` table has an index on `parent_id`.
- `013.18` -- The `snapshot` table has an index on `last_seen`.
- `013.19` -- The `snapshot` table has an index on `deleted_time`.
- `013.20` -- The `snapshot` table holds at most one row per tracked path.

## Notes
The plan's phrase "informational only for directories" for `mod_time` describes
how `mod_time` is used in directory sync decisions, which is decision behavior
owned by other categories and not observable in the schema; only `mod_time`'s
schema presence, type, and not-null constraint are asserted here.
