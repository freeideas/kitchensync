# Path observation record store

## Purpose

A per-tree database that records observation history for each path in a directory tree. For each path observed, the store remembers the path's most recently observed modification time and byte size, when it was last confirmed present, and (if currently absent) when its disappearance was first noted. Storage is a single SQLite database file containing exactly one table. The store also exposes a deterministic path-identity function over UTF-8 path strings and a process-monotonic UTC timestamp generator in a canonical filesystem-safe format. Pure data-layer library — no networking, no concurrency primitives beyond what SQLite itself provides.

## API surface

### Store lifecycle

- `open(file)` → handle — open the database at the given filesystem path. If the file does not exist, create it and initialize its schema. Subsequent opens reuse the existing file.
- `close(handle)` — close the database.

### Record shape

A record represents one path's observation history:

| Field          | Type                       | Notes                                                                                                                                              |
| -------------- | -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`           | 11-char string             | Path identity for this path (see §"Path identity")                                                                                                  |
| `parent_id`    | 11-char string             | Path identity of this path's parent directory; for top-level entries, the identity of the sentinel root `/`                                         |
| `basename`     | string                     | The path's final component                                                                                                                          |
| `mod_time`     | timestamp string           | Last observed modification time (see §"Timestamps")                                                                                                 |
| `byte_size`    | integer                    | File size in bytes for files; `-1` for directories                                                                                                  |
| `last_seen`    | timestamp string or null   | The most recent time the path was confirmed present. Null when a decision has been recorded for the path but its presence has not yet been confirmed |
| `deleted_time` | timestamp string or null   | Null while the path is considered present. Non-null timestamp string when the path is tombstoned                                                    |

The on-disk table is named `snapshot` (singular, lowercase). It is the only table the library creates or queries. Indexes exist on `parent_id`, `last_seen`, and `deleted_time`.

### Record operations

- `upsert_observed(handle, path, mod_time, byte_size, is_dir, now)` — record that `path` was directly observed present. Insert or update the row with `mod_time`, `byte_size` (`-1` if `is_dir`, the file size otherwise), `last_seen = now`, and `deleted_time = null`. `basename` and `parent_id` are derived from `path` by the library; the caller does not supply them.
- `record_decided(handle, path, mod_time, byte_size, is_dir)` — record that a decision has been made about `path` but its presence is not yet confirmed (e.g., a transfer has been chosen but has not yet completed). Insert or update the row with `mod_time`, `byte_size`, and `deleted_time = null`. **Do not** modify `last_seen` — leave it null on insert and unchanged on update.
- `confirm_present(handle, path, now)` — set `last_seen = now` on the existing row at `path`; other fields unchanged. No-op if no row exists.
- `mark_subtree_deleted(handle, path, deleted_time)` — atomically set `deleted_time` on the row at `path` and on every transitive descendant whose current `deleted_time` is null. The descendant chain is followed through the `parent_id → id` relationship rooted at `path`'s identity. Rows whose `deleted_time` is already non-null are left untouched. If no row exists at `path`, no-op.
- `lookup(handle, path)` → record or none
- `list_children(handle, parent_path)` → list of records — every row whose `parent_id` equals the identity of `parent_path`. The caller may pass `/` (or the empty string) to list the root's immediate children.

### Purge

- `purge_older_than(handle, retention_days, now)` — delete every row in either of these classes:
  - tombstone rows (`deleted_time` non-null) whose `deleted_time` is older than `retention_days` calendar days before `now`;
  - non-tombstone rows whose `last_seen` is older than `retention_days` calendar days before `now`, or whose `last_seen` is null.

### Path identity

- `identify(relative_path)` → 11-character string
  - Input is forward-slash-delimited, no leading or trailing slash, no `.` or `..` components. UTF-8 encoded.
  - Files and directories at the same path produce the same identity; the record's `byte_size = -1` is what marks a directory.
  - The empty string and `/` both denote the **root sentinel** identity (used as `parent_id` for top-level entries).
  - Hash the UTF-8 bytes of the input with xxHash64 (seed 0). Encode the resulting 64-bit value as base62 using the alphabet `0-9`, `A-Z`, `a-z` in that order. Zero-pad to exactly 11 characters.

The identity is stable across processes, machines, and runs — callers may rely on it being portable.

### Timestamps

- `now()` → timestamp string. Returns the current UTC wall-clock time formatted as `YYYY-MM-DD_HH-mm-ss_ffffffZ` — four-digit year, two-digit month, two-digit day, underscore, two-digit hour, two-digit minute, two-digit second, underscore, six-digit microseconds, literal `Z`. All numeric fields are zero-padded. The resulting string is lexicographically sortable and uses only characters that are valid in path components on common operating systems.
- The generator is **process-monotonic**: every call returns a value strictly greater than every value previously returned by `now()` in the same process. If the wall clock has not advanced past the most recently returned value, the generator returns the most recent value plus one microsecond.

A caller that needs multiple distinct "current time" values within one logical operation must call `now()` afresh for each one — the library will return distinct, monotonically increasing values.

## Anchoring

- **Database storage**: SQLite — file-based, embedded, ACID. One database file per store; one table named `snapshot`.
- **Recursive descendant traversal**: a recursive Common Table Expression — a standard SQL feature supported by SQLite.
- **Path hashing**: xxHash64 with seed `0` over the UTF-8 byte representation of the input string. The xxHash family is a well-documented external standard.
- **Numeric encoding**: base62 with the alphabet `0-9`, then `A-Z`, then `a-z` — a standard positional numeral system.
- **Timestamp format**: a UTC ISO-8601-derived format using only path-safe punctuation (`-`, `_`, `Z`). Microsecond precision; lexicographic sort order matches chronological order.
- **Monotonic clock**: a standard concurrency primitive — every value returned strictly greater than the previous one returned in the same process.
- "Path", "directory", "parent directory", "basename", "relative path": host-language string and filesystem-path primitives. Forward-slash-delimited; no `.` or `..`; no leading or trailing slash.
- "Record", "row", "table", "transaction": SQL primitives.
