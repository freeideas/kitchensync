# Snapshot Database

SQLite snapshot database schema, path hashing, timestamps, URL normalization, and tombstone management.

## $REQ_DB_001: SQLite Database Location
**Source:** ./specs/database.md (Section: "Database")

Each peer stores its own snapshot in `{peer-root}/.kitchensync/snapshot.db`.

## $REQ_DB_002: SQLite WAL Mode
**Source:** ./specs/database.md (Section: "Database")

The snapshot database uses SQLite WAL mode.

## $REQ_DB_003: Foreign Keys Enabled
**Source:** ./specs/database.md (Section: "Database")

The snapshot database has foreign keys enabled.

## $REQ_DB_004: Local Copy Workflow
**Source:** ./specs/database.md (Section: "Database")

At the start of a run, each peer's `snapshot.db` is downloaded to a local temporary directory (`{tmp}/{uuid}/snapshot.db`). All reads and writes happen against this local copy.

## $REQ_DB_005: Atomic Snapshot Upload
**Source:** ./specs/database.md (Section: "Database")

After sync completes, the updated database is written back atomically: upload as `snapshot-new.db`, then rename to `snapshot.db`.

## $REQ_DB_006: New Database for Missing Snapshot
**Source:** ./specs/database.md (Section: "Database")

If a peer has no existing `snapshot.db`, a new one is created locally.

## $REQ_DB_007: Snapshot Table Schema
**Source:** ./specs/database.md (Section: "Snapshot")

The snapshot table has columns: `id` (TEXT, primary key — xxHash64 of relative path, base62-encoded, 11 chars), `parent_id` (TEXT — xxHash64 of parent directory's relative path, base62-encoded), `basename` (TEXT, not null), `mod_time` (TEXT, not null — `YYYY-MM-DD_HH-mm-ss_ffffffZ`), `byte_size` (INTEGER, not null — bytes for files, -1 for directories), `last_seen` (TEXT or NULL), `deleted_time` (TEXT or NULL).

## $REQ_DB_008: Snapshot Table Indexes
**Source:** ./specs/database.md (Section: "Snapshot")

The snapshot table has indexes on `parent_id`, `last_seen`, and `deleted_time`.

## $REQ_DB_009: Root Entry Parent Sentinel
**Source:** ./specs/database.md (Section: "Path Hashing")

Root entries use the hash of `/` as the `parent_id` sentinel value.

## $REQ_DB_010: Path Hashing Algorithm
**Source:** ./specs/database.md (Section: "Path Hashing")

Paths are hashed with xxHash64 (seed 0) and encoded as base62 (digits `0-9`, uppercase `A-Z`, lowercase `a-z`). 64 bits → 11 characters, zero-padded.

## $REQ_DB_011: Path Hashing Format
**Source:** ./specs/database.md (Section: "Path Hashing")

Paths use forward slashes, no leading slash, no trailing slash. Files and directories are hashed identically; `byte_size = -1` distinguishes directories.

## $REQ_DB_012: Sync Root Not Tracked
**Source:** ./specs/database.md (Section: "Path Hashing")

The sync root directory itself is not tracked in the snapshot — only its children are.

## $REQ_DB_013: Timestamp Format
**Source:** ./specs/database.md (Section: "Timestamps")

All timestamps use the format `YYYY-MM-DD_HH-mm-ss_ffffffZ` — UTC, microsecond precision, lexicographic sort, filesystem-safe. This format is used in database columns, BAK/ directory names, TMP/ directory names, and log output.

## $REQ_DB_014: Monotonic Timestamps
**Source:** ./specs/database.md (Section: "Timestamps")

Timestamps are monotonic within a process: 1μs is added on collision.

## $REQ_DB_015: URL Normalization - Lowercase Scheme and Host
**Source:** ./specs/database.md (Section: "URL Normalization")

URLs are normalized by lowercasing the scheme and hostname.

## $REQ_DB_016: URL Normalization - Remove Default Port
**Source:** ./specs/database.md (Section: "URL Normalization")

URL normalization removes the default port (22 for SFTP).

## $REQ_DB_017: URL Normalization - Collapse Slashes
**Source:** ./specs/database.md (Section: "URL Normalization")

URL normalization collapses consecutive slashes in the path.

## $REQ_DB_018: URL Normalization - Remove Trailing Slash
**Source:** ./specs/database.md (Section: "URL Normalization")

URL normalization removes trailing slashes from the path.

## $REQ_DB_019: URL Normalization - Bare Paths to file://
**Source:** ./specs/database.md (Section: "URL Normalization")

Bare paths (no scheme) are converted to `file://` URLs.

## $REQ_DB_020: URL Normalization - file:// Resolve to Absolute
**Source:** ./specs/database.md (Section: "URL Normalization")

`file://` URLs are resolved to absolute paths from the current working directory.

## $REQ_DB_021: URL Normalization - Percent-Decode Unreserved
**Source:** ./specs/database.md (Section: "URL Normalization")

URL normalization percent-decodes unreserved characters.

## $REQ_DB_022: URL Normalization - Strip Query Parameters
**Source:** ./specs/database.md (Section: "URL Normalization")

URL normalization strips query-string parameters (per-URL settings like `?mc=5` are not part of the identity).

## $REQ_DB_023: Tombstone Creation
**Source:** ./specs/database.md (Section: "Tombstones")

When an entry is confirmed absent on a peer where a snapshot row exists with `deleted_time = NULL`, the row is retained and `deleted_time` is set to the current value of `last_seen`.

## $REQ_DB_024: Tombstone Purge
**Source:** ./specs/database.md (Section: "Tombstones")

Tombstones are purged when `deleted_time` is older than `--td` days (default: 180).

## $REQ_DB_025: Stale Row Purge
**Source:** ./specs/multi-tree-sync.md (Section: "Orphaned Snapshot Rows")

Rows where `deleted_time IS NULL` and `last_seen` is older than `--td` days (or `last_seen` is NULL) are also deleted during the startup purge.
