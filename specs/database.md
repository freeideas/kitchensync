# Database

Single SQLite database. Location set by `database` in config (default: `kitchensync.db` in the config file's directory). If the database value is an absolute path, it is used as-is. If relative, it resolves from the config file's directory. WAL mode. Foreign keys enabled.

## Schema

```sql
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS applog (
    log_id INTEGER PRIMARY KEY,
    stamp TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_applog_stamp ON applog(stamp);

CREATE TABLE IF NOT EXISTS snapshot (
    id BLOB PRIMARY KEY,
    parent_id BLOB NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT NOT NULL,
    byte_size INTEGER,
    del_time TEXT
);
CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_del ON snapshot(del_time);
```

## Snapshot

Represents the decided state — what all peers should look like after sync.

- **id**: xxHash64 of full relative path (forward slashes), 8 raw bytes
- **parent_id**: xxHash64 of parent path with trailing `/`. Root entries use hash of `/`.
- **basename**: final path component
- **mod_time**: `YYYYMMDDTHHmmss.ffffffZ` for files and directories
- **byte_size**: bytes for files, -1 for directories
- **del_time**: NULL if live, timestamp if deleted (tombstone)

Updated during traversal, before file copies complete. If copies don't finish, next run detects the discrepancy and re-enqueues.

## Tombstones

When the traversal decides a file is deleted, `del_time` is set. The row remains so future runs can distinguish "was deleted" from "never existed." Purged after `tombstone-retention-days` (default: 180).

## Path Hashing

- Forward slashes, no leading slash
- Trailing slash for directories and parent paths
- `docs/readme.txt` → hash of `docs/readme.txt`
- `docs/notes/` (dir) → hash of `docs/notes/`
- Parent of `docs/readme.txt` → hash of `docs/`
- Parent of root entries → hash of `/`

xxHash64 with seed 0. Fast, well-distributed, 8 bytes.

## Timestamps

Format: `YYYYMMDDTHHmmss.ffffffZ` — UTC, microsecond precision, lexicographic sort.

Monotonic within a process: add 1μs on collision.
