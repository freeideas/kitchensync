# 02_upsert-observed: Recording a directly observed path inserts or updates its row

## Behavior
`upsert_observed(handle, path, mod_time, byte_size, is_dir, now)` records that the caller directly observed `path` to be present. It inserts a new row when none exists at `path`, or updates the existing row, in either case writing `mod_time`, `byte_size` (overridden to `-1` when `is_dir` is true), `last_seen = now`, and `deleted_time = null` — clearing any prior tombstone. The library derives the row's `basename` from the path's final component and its `parent_id` from `identify` of the parent directory, with the root sentinel identity used for top-level entries. Derived from `./specs/SPEC.md` § "Record operations" and § "Record shape".

## $REQ_IDs
- `02.1` — `upsert_observed` inserts a new row when no row exists at `path`.
- `02.2` — A row written by `upsert_observed` has the supplied `mod_time`, `last_seen` equal to the supplied `now`, and `deleted_time` null.
- `02.3` — Calling `upsert_observed` on an existing row updates that row's `mod_time`, `byte_size`, and `last_seen` rather than inserting a new row.
- `02.4` — Calling `upsert_observed` on a tombstoned row (non-null `deleted_time`) clears `deleted_time` back to null.
- `02.5` — When `is_dir` is true, the stored `byte_size` is `-1`; when `is_dir` is false, the stored `byte_size` is the supplied value.
- `02.6` — The row's `basename` equals the final path component of the supplied `path`.
- `02.7` — The row's `parent_id` equals `identify(parent_directory_of_path)`.
- `02.8` — A row written for a top-level entry (no parent directory) has `parent_id` equal to the root-sentinel identity returned by `identify("")` / `identify("/")`.
