# Database

SQLite database creation, schema, and operational modes.

## $REQ_DB_001: Database Location
**Source:** ./specs/database.md (Section: "Database")

The database is a single SQLite file named `kitchensync.db` inside the config directory (default `~/.kitchensync/`). The database path is not separately configurable.

## $REQ_DB_002: WAL Mode
**Source:** ./specs/database.md (Section: "Database")

The database uses WAL mode.

## $REQ_DB_003: Foreign Keys Enabled
**Source:** ./specs/database.md (Section: "Database")

Foreign keys are enabled in the database.

## $REQ_DB_004: Config Table
**Source:** ./specs/database.md (Section: "Schema")

The database contains a `config` table with columns `key` (TEXT PRIMARY KEY) and `value` (TEXT NOT NULL).

## $REQ_DB_005: Applog Table
**Source:** ./specs/database.md (Section: "Schema")

The database contains an `applog` table with columns `log_id` (INTEGER PRIMARY KEY), `stamp` (TEXT NOT NULL), `level` (TEXT NOT NULL), and `message` (TEXT NOT NULL), with an index on `stamp`.

## $REQ_DB_006: Peer Table
**Source:** ./specs/database.md (Section: "Schema")

The database contains a `peer` table with column `peer_id` (INTEGER PRIMARY KEY).

## $REQ_DB_007: Peer URL Table
**Source:** ./specs/database.md (Section: "Schema")

The database contains a `peer_url` table with columns `peer_id` (INTEGER NOT NULL, FK to peer) and `normalized_url` (TEXT NOT NULL UNIQUE), with a composite primary key on `(peer_id, normalized_url)`.

## $REQ_DB_008: Snapshot Table
**Source:** ./specs/database.md (Section: "Schema")

The database contains a `snapshot` table with columns `id` (TEXT NOT NULL), `peer_id` (INTEGER NOT NULL, FK to peer), `parent_id` (TEXT NOT NULL), `basename` (TEXT NOT NULL), `mod_time` (TEXT NOT NULL), `byte_size` (INTEGER NOT NULL), `last_seen` (TEXT nullable), and `deleted_time` (TEXT nullable), with a composite primary key on `(id, peer_id)` and indexes on `parent_id`, `last_seen`, and `deleted_time`.

## $REQ_DB_009: Timestamp Format
**Source:** ./specs/database.md (Section: "Timestamps")

All timestamps use the format `YYYY-MM-DD_HH-mm-ss_ffffffZ` — UTC, microsecond precision, lexicographically sortable, and filesystem-safe. This format is used in database columns, BACK/ directory names, XFER/ directory names, and log entries.

## $REQ_DB_010: Monotonic Timestamps
**Source:** ./specs/database.md (Section: "Timestamps")

Timestamps are monotonic within a process: 1μs is added on collision.
