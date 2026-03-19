# Database

SQLite database setup, schema, path hashing, and timestamp format.

## $REQ_DB_001: SQLite Database
**Source:** ./specs/database.md (Section: "Database")

The application uses a single SQLite database. Its location is set by the `database` config key (default: `kitchensync.db` in the config file's directory).

## $REQ_DB_002: Database Path Resolution
**Source:** ./specs/database.md (Section: "Database")

If the database path is absolute, it is used as-is. If relative, it resolves from the config file's directory.

## $REQ_DB_003: WAL Mode
**Source:** ./specs/database.md (Section: "Database")

The database is opened in WAL mode.

## $REQ_DB_004: Foreign Keys Enabled
**Source:** ./specs/database.md (Section: "Database")

Foreign keys are enabled on the database connection.

## $REQ_DB_005: Config Table Schema
**Source:** ./specs/database.md (Section: "Schema")

The database contains a `config` table with columns `key` (TEXT PRIMARY KEY) and `value` (TEXT NOT NULL).

## $REQ_DB_006: Applog Table Schema
**Source:** ./specs/database.md (Section: "Schema")

The database contains an `applog` table with columns `log_id` (INTEGER PRIMARY KEY), `stamp` (TEXT NOT NULL), `level` (TEXT NOT NULL), and `message` (TEXT NOT NULL), with an index on `stamp`.

## $REQ_DB_007: Snapshot Table Schema
**Source:** ./specs/database.md (Section: "Schema")

The database contains a `snapshot` table with columns `id` (BLOB NOT NULL), `peer` (TEXT NOT NULL), `parent_id` (BLOB NOT NULL), `basename` (TEXT NOT NULL), `mod_time` (TEXT NOT NULL), `byte_size` (INTEGER NOT NULL), `last_seen` (TEXT nullable), `deleted_time` (TEXT nullable), with primary key `(id, peer)` and indexes on `parent_id`, `last_seen`, and `deleted_time`.

## $REQ_DB_008: Path ID Hashing
**Source:** ./specs/database.md (Section: "Path Hashing")

The `id` field is xxHash64 (seed 0) of the full relative path using forward slashes, stored as 8 raw bytes. Files have no trailing slash; directories and parent paths have a trailing slash.

## $REQ_DB_009: Parent ID Hashing
**Source:** ./specs/database.md (Section: "Path Hashing")

The `parent_id` field is xxHash64 of the parent path with a trailing slash. Root entries use the hash of `/`.

## $REQ_DB_010: Root Not Tracked
**Source:** ./specs/database.md (Section: "Path Hashing")

The sync root directory itself is not tracked in the snapshot — only its children are.

## $REQ_DB_011: Timestamp Format
**Source:** ./specs/database.md (Section: "Timestamps")

All timestamps use the format `YYYYMMDDTHHmmss.ffffffZ` — UTC with microsecond precision, lexicographically sortable.

## $REQ_DB_012: Monotonic Timestamps
**Source:** ./specs/database.md (Section: "Timestamps")

Timestamps are monotonic within a process: 1 microsecond is added on collision.

## $REQ_DB_013: Snapshot Byte Size Convention
**Source:** ./specs/database.md (Section: "Snapshot")

The `byte_size` field stores the file size in bytes for files, or −1 for directories.

## $REQ_DB_014: Last Seen Semantics
**Source:** ./specs/database.md (Section: "Snapshot")

The `last_seen` field is set to the current sync timestamp when the entry is confirmed present on a peer (via listing or after a completed copy). It is NULL when a push has been decided but the copy has not yet completed.

## $REQ_DB_015: Deleted Time Semantics
**Source:** ./specs/database.md (Section: "Snapshot")

The `deleted_time` field is NULL while the entry exists (or a copy is pending). It is set when the entry is confirmed absent on a peer. The value is copied from `last_seen` at the time of detection.

## $REQ_DB_016: Tombstone Purging
**Source:** ./specs/database.md (Section: "Tombstones")

Snapshot rows with `deleted_time IS NOT NULL` (tombstones) are purged when `deleted_time` is older than `tombstone-retention-days` (default: 180 days).
