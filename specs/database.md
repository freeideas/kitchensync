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
    id BLOB NOT NULL,
    peer TEXT NOT NULL,
    parent_id BLOB NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    last_seen TEXT,
    deleted_time TEXT,
    PRIMARY KEY (id, peer)
);
CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_last_seen ON snapshot(last_seen);
CREATE INDEX IF NOT EXISTS idx_snapshot_deleted ON snapshot(deleted_time);
```

## Snapshot

Tracks per-peer state — one row per path per peer that has (or had) the entry.

- **id**: xxHash64 of full relative path (forward slashes), 8 raw bytes
- **peer**: peer name from config
- **parent_id**: xxHash64 of parent path with trailing `/`. Root entries use hash of `/`.
- **basename**: final path component
- **mod_time**: `YYYYMMDDTHHmmss.ffffffZ` — the entry's mod_time as last observed on this peer
- **byte_size**: bytes for files, -1 for directories
- **last_seen**: `YYYYMMDDTHHmmss.ffffffZ` or NULL — set to the current sync timestamp when the entry is confirmed present on this peer (via listing or after a completed copy). NULL when a push has been decided but the copy has not yet completed. Only confirmed presence updates this field.
- **deleted_time**: `YYYYMMDDTHHmmss.ffffffZ` or NULL — NULL while the entry exists (or a copy is pending). Set when the entry is confirmed absent on this peer. The value is copied from `last_seen` at the time of detection (a conservative estimate — the real deletion happened sometime after `last_seen`).

Updated during traversal, before file copies complete, except for `last_seen` on copy destinations — that is set after the copy completes. If copies don't finish, the destination row has `deleted_time = NULL` and `last_seen` unchanged (NULL for first-time targets). The next run applies rule 4b: since `last_seen` is NULL or old, it does not exceed the source's mod_time, so the copy is re-enqueued.

## Tombstones

When a file is confirmed absent on a peer where a snapshot row exists with `deleted_time = NULL`, the row is retained and `deleted_time` is set to the current value of `last_seen` (a conservative estimate — the real deletion happened sometime after that). A row with `deleted_time IS NOT NULL` is a tombstone. Tombstones are purged when `deleted_time` is older than `tombstone-retention-days` (default: 180).

## Path Hashing

- Forward slashes, no leading slash
- Trailing slash for directories and parent paths
- `docs/readme.txt` → hash of `docs/readme.txt`
- `docs/notes/` (dir) → hash of `docs/notes/`
- Parent of `docs/readme.txt` → hash of `docs/`
- Parent of root entries → hash of `/`
- The sync root directory itself is not tracked in the snapshot — only its children are. Traversal begins by listing the root; the root has no snapshot row.

xxHash64 with seed 0. Fast, well-distributed, 8 bytes.

## Timestamps

Format: `YYYYMMDDTHHmmss.ffffffZ` — UTC, microsecond precision, lexicographic sort.

Monotonic within a process: add 1μs on collision.
