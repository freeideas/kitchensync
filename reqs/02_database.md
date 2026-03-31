# Database

Snapshot database schema, timestamps, path hashing, and tombstone management.

## $REQ_DB_001: Snapshot Location
**Source:** ./specs/database.md (Section: "Database")

Each peer stores its snapshot in `{peer-root}/.kitchensync/snapshot.db`.

## $REQ_DB_002: SQLite with WAL Mode
**Source:** ./specs/database.md (Section: "Schema")

The snapshot database is SQLite with WAL journal mode and foreign keys enabled.

## $REQ_DB_003: Snapshot Schema
**Source:** ./specs/database.md (Section: "Schema")

The snapshot table has columns: `id` (TEXT PRIMARY KEY, xxHash64 base62), `parent_id` (TEXT NOT NULL, foreign key to snapshot.id), `basename` (TEXT NOT NULL), `mod_time` (TEXT NOT NULL), `byte_size` (INTEGER NOT NULL, -1 for directories), `last_seen` (TEXT, nullable), `deleted_time` (TEXT, nullable). Indexes exist on `parent_id`, `last_seen`, and `deleted_time`.

## $REQ_DB_004: Timestamp Format
**Source:** ./specs/database.md (Section: "Timestamps")

All timestamps use the format `YYYY-MM-DD_HH-mm-ss_ffffffZ` — UTC, microsecond precision, lexicographically sortable, filesystem-safe. This format is used in database columns, BAK/ directory names, TMP/ directory names, and log output.

## $REQ_DB_005: Monotonic Timestamps
**Source:** ./specs/database.md (Section: "Timestamps")

Timestamps are monotonic within a process: 1 microsecond is added on collision. A single process-global generator is used for all BAK directory names, TMP directory names, and database timestamps.

## $REQ_DB_006: Path Hashing
**Source:** ./specs/database.md (Section: "Path Hashing")

Paths are hashed with xxHash64 (seed 0) and encoded as base62 (digits 0-9, uppercase A-Z, lowercase a-z), zero-padded to 11 characters. Most-significant digit first.

## $REQ_DB_007: Path Hashing Rules
**Source:** ./specs/database.md (Section: "Path Hashing")

Paths use forward slashes, no leading slash, no trailing slash. Files and directories hash identically (`byte_size = -1` distinguishes directories). Parent of root entries is the hash of `/` (sentinel).

## $REQ_DB_008: Sentinel Row
**Source:** ./specs/database.md (Section: "Path Hashing")

A new snapshot database contains a sentinel row so that root-level entries satisfy the foreign key on `parent_id`. The sentinel's `parent_id` references itself, with basename `''`, mod_time `'0000-00-00_00-00-00_000000Z'`, and byte_size `-1`.

## $REQ_DB_009: Local Working Copy
**Source:** ./specs/database.md (Section: "Database")

At the start of a run, each peer's `snapshot.db` is downloaded to a local temporary directory. All reads and writes happen against this local copy. After sync completes, the updated database is written back using TMP staging.

## $REQ_DB_010: New Peer Gets Empty Snapshot
**Source:** ./specs/database.md (Section: "Database")

If a peer has no existing `snapshot.db`, a new one is created locally.

## $REQ_DB_011: Tombstone Creation
**Source:** ./specs/database.md (Section: "Tombstones")

When an entry is confirmed absent on a peer where a snapshot row exists with `deleted_time = NULL`, the row is retained and `deleted_time` is set to the current value of `last_seen`.

## $REQ_DB_012: Sync Root Not Tracked
**Source:** ./specs/database.md (Section: "Path Hashing")

The sync root directory itself is not tracked in the snapshot — only its children. Traversal begins by listing the root; the root has no snapshot row.
