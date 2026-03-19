# Database

SQLite database schema, path hashing, snapshot representation, and timestamp format.

## $REQ_DB_001: SQLite Database Created or Opened
**Source:** ./specs/database.md (Section: "Database")

A single SQLite database is used. It is created if it does not exist, or opened if it does.

## $REQ_DB_002: Database Location Default
**Source:** ./specs/database.md (Section: "Database")

The database location is set by the `database` config setting. Default: `kitchensync.db` in the config file's directory.

## $REQ_DB_003: Absolute Database Path
**Source:** ./specs/database.md (Section: "Database")

If the `database` value is an absolute path, it is used as-is.

## $REQ_DB_004: Relative Database Path
**Source:** ./specs/database.md (Section: "Database")

If the `database` value is a relative path, it resolves from the config file's directory.

## $REQ_DB_005: WAL Mode
**Source:** ./specs/database.md (Section: "Database")

The database uses WAL mode.

## $REQ_DB_006: Foreign Keys Enabled
**Source:** ./specs/database.md (Section: "Database")

Foreign keys are enabled on the database.

## $REQ_DB_007: Config Table Schema
**Source:** ./specs/database.md (Section: "Schema")

The `config` table exists with columns: `key` (TEXT PRIMARY KEY), `value` (TEXT NOT NULL).

## $REQ_DB_008: Applog Table Schema
**Source:** ./specs/database.md (Section: "Schema")

The `applog` table exists with columns: `log_id` (INTEGER PRIMARY KEY), `stamp` (TEXT NOT NULL), `level` (TEXT NOT NULL), `message` (TEXT NOT NULL). An index exists on `stamp`.

## $REQ_DB_009: Snapshot Table Schema
**Source:** ./specs/database.md (Section: "Schema")

The `snapshot` table exists with columns: `id` (BLOB PRIMARY KEY), `parent_id` (BLOB NOT NULL), `basename` (TEXT NOT NULL), `mod_time` (TEXT NOT NULL), `byte_size` (INTEGER), `del_time` (TEXT). Indexes exist on `parent_id` and `del_time`.

## $REQ_DB_010: Snapshot ID
**Source:** ./specs/database.md (Section: "Path Hashing")

The snapshot `id` is the xxHash64 (seed 0) of the full relative path using forward slashes (e.g., `docs/readme.txt` for a file, `docs/notes/` for a directory).

## $REQ_DB_011: Snapshot Parent ID
**Source:** ./specs/database.md (Section: "Path Hashing")

The snapshot `parent_id` is the xxHash64 of the parent path with a trailing slash. Root entries use the hash of `/`.

## $REQ_DB_012: Timestamp Format
**Source:** ./specs/database.md (Section: "Timestamps")

All timestamps use `YYYYMMDDTHHmmss.ffffffZ` — UTC with microsecond precision. Timestamps sort lexicographically.

## $REQ_DB_013: Monotonic Timestamps
**Source:** ./specs/database.md (Section: "Timestamps")

Timestamps are monotonic within a process: 1 microsecond is added on collision.

## $REQ_DB_014: Tombstone Representation
**Source:** ./specs/database.md (Section: "Tombstones")

When a file is deleted, its snapshot row's `del_time` is set to a timestamp. The row remains so future runs distinguish "was deleted" from "never existed." `del_time` is NULL for live entries.

## $REQ_DB_015: Directory Byte Size
**Source:** ./specs/database.md (Section: "Snapshot")

Directories have `byte_size` of -1. Files have `byte_size` in bytes.

## $REQ_DB_016: Basename Is Final Path Component
**Source:** ./specs/database.md (Section: "Snapshot")

The snapshot `basename` stores the final path component.
