# Snapshot Database

SQLite snapshot database schema, timestamps, path hashing, and tombstone management.

## $REQ_DB_001: Snapshot Location
**Source:** ./specs/database.md (Section: top)

Each peer stores its snapshot in `{peer-root}/.kitchensync/snapshot.db`, a SQLite database in WAL mode with foreign keys enabled.

## $REQ_DB_002: Snapshot Schema
**Source:** ./specs/database.md (Section: "Schema")

The snapshot table has columns: `id` (TEXT PRIMARY KEY, xxHash64 base62-encoded), `parent_id` (TEXT NOT NULL, foreign key to snapshot.id), `basename` (TEXT NOT NULL), `mod_time` (TEXT NOT NULL), `byte_size` (INTEGER NOT NULL, -1 for directories), `last_seen` (TEXT nullable), `deleted_time` (TEXT nullable). Indexes exist on `parent_id`, `last_seen`, and `deleted_time`.

## $REQ_DB_003: Timestamp Format
**Source:** ./specs/database.md (Section: "Timestamps")

All timestamps use the format `YYYY-MM-DD_HH-mm-ss_ffffffZ` -- UTC, microsecond precision, lexicographic sort, filesystem-safe. This format is used in database columns, BAK/ directory names, TMP/ directory names, and log output.

## $REQ_DB_004: Monotonic Timestamps
**Source:** ./specs/database.md (Section: "Timestamps")

Timestamps are monotonic within a process: 1 microsecond is added on collision. A single process-global generator is used for all BAK, TMP, and database timestamps.

## $REQ_DB_005: Path Hashing
**Source:** ./specs/database.md (Section: "Path Hashing")

Paths are hashed with xxHash64 (seed 0) and encoded as base62 (0-9, A-Z, a-z), zero-padded to 11 characters. Forward slashes only, no leading or trailing slash.

## $REQ_DB_006: Sentinel Row
**Source:** ./specs/database.md (Section: "Path Hashing")

A sentinel row is inserted when creating a new snapshot database so root-level entries satisfy the foreign key on `parent_id`. The sentinel's `parent_id` references itself, with basename `''`, mod_time `0000-00-00_00-00-00_000000Z`, and byte_size `-1`.

## $REQ_DB_007: Tombstone Creation
**Source:** ./specs/database.md (Section: "Tombstones")

When an entry is confirmed absent on a peer where a snapshot row exists with `deleted_time = NULL`, the row is retained and `deleted_time` is set to the current value of `last_seen`.

## $REQ_DB_008: Tombstone Purging
**Source:** ./specs/database.md (Section: "Tombstones")

Tombstones (rows with `deleted_time IS NOT NULL`) are purged when `deleted_time` is older than `--td` days (default: 180). Non-tombstone rows with `last_seen` older than `--td` days are also purged, except rows with `last_seen = NULL` (pending copies).

## $REQ_DB_009: Tombstone Purging Disabled
**Source:** ./specs/algorithm.md (Section: "Startup")

When `--td` is set to 0, tombstone purging is skipped entirely.

## $REQ_DB_010: OS-Native Paths for file:// URLs
**Source:** ./specs/database.md (Section: "OS-Native Paths for file:// URLs")

On Windows, the normalized URL path has a leading slash (`/c:/photos`), but filesystem calls require native format (`c:/photos`). A separate accessor strips the leading slash on Windows drive-letter paths for all filesystem operations.
