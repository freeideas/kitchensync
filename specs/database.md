# Database

KitchenSync stores all metadata in SQLite databases. There is one local database and one temporary database per reachable peer.

## Local Database

Location: `.kitchensync/kitchensync.db`

Contains:
- **snapshot** table -- the local filesystem's known state
- **config** table -- runtime config (serving port, last_walk_time)
- **applog** table -- application logs

The local database is persistent and survives across runs.

## Peer Databases

Location: `.kitchensync/PEER/{peer-name}.db`

Each configured peer has a database containing:
- **snapshot** table -- the peer's filesystem state, updated incrementally on each connection
- **queue** table -- paths awaiting sync, persists across runs
- **config** table -- peer-specific runtime config (`last_walk_time`)

The peer snapshot reflects the peer's **actual** state, not the expected state after queued transfers complete. It is updated only after a transfer succeeds. Why? If a transfer fails mid-way, the snapshot remains accurate -- we don't falsely believe the peer has a file it doesn't. This may cause redundant enqueues during slow transfers, but redundant enqueues are harmless (deduped by path, caught at recheck). The alternative -- optimistically updating before transfer -- would leave the database wrong on failure, causing files to silently not sync.

Queue entries survive restarts and disconnections, enabling fast sync when a peer reconnects. At startup, peer databases for peers not listed in `peers.conf` are deleted -- this cleans up after peer removal.

### Peer Snapshot Persistence

The peer snapshot table persists across connections, just like the local snapshot. This enables accurate deletion detection: when a file exists in the snapshot but not on the peer filesystem, we know it was deleted and can set `del_time` to `last_walk_time` (conservative estimate). Without persistence, we couldn't distinguish "file was deleted" from "file never existed" -- deletions on the peer would never propagate.

The `last_walk_time` is stored in the peer database's config table and updated at the end of each successful peer walk.

## Snapshot Table

```sql
CREATE TABLE IF NOT EXISTS snapshot (
    id BLOB PRIMARY KEY,        -- hash of full relative path
    parent_id BLOB NOT NULL,    -- hash of parent path (with trailing /)
    basename TEXT NOT NULL,     -- final path component
    mod_time TEXT,              -- file: ISO timestamp; directory: NULL
    byte_size INTEGER,          -- file: size in bytes; directory: -1
    del_time TEXT               -- NULL if not deleted; ISO timestamp if deleted
);

CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_del ON snapshot(del_time);
```

### Column Semantics

**id**: xxHash64 of the full relative path using forward-slash separators, stored as 8 bytes. Example: the path `docs/notes/readme.txt` hashes to produce the id.

**parent_id**: xxHash64 of the parent path including the trailing slash. For `docs/notes/readme.txt`, the parent_id is the hash of `docs/notes/`. For a file at the root (e.g. `readme.txt`), the parent_id is the hash of `/`. The sync root directory itself has no row in the snapshot table -- it exists implicitly.

**basename**: The final component of the path. For `docs/notes/readme.txt`, the basename is `readme.txt`.

**mod_time**: For files, the modification timestamp in ISO format (`YYYYMMDDTHHmmss.ffffffZ`). For directories, always NULL -- directory modification times are not meaningful for sync purposes.

**byte_size**: For files, the size in bytes. For directories, always -1. This distinguishes files from directories.

**del_time**: NULL if the entry exists (not deleted). If deleted, contains the deletion timestamp in ISO format. This replaces the old tombstone files.

### Path Hashing

Paths are normalized before hashing:
- Use forward slashes as separators (even on Windows)
- No leading slash
- No trailing slash for files
- Trailing slash for directories and parent paths
- Case is preserved exactly as returned by the filesystem

Examples:
- File `docs/readme.txt` -- hash of `docs/readme.txt`
- Directory `docs/notes` -- hash of `docs/notes/`
- Parent of `docs/readme.txt` -- hash of `docs/`
- Parent of `readme.txt` (at root) -- hash of `/`

### Hash Algorithm

xxHash64 is used for path hashing. It is:
- **Fast** -- one of the fastest non-cryptographic hashes available
- **Well-distributed** -- low collision rate for typical path strings
- **Widely supported** -- Rust has `xxhash-rust` crate; most languages have bindings

The hash output is stored as 8 raw bytes (BLOB), not hex-encoded. This saves space and makes comparisons faster.

### Detecting Entry Type

```sql
-- Files only
SELECT * FROM snapshot WHERE byte_size >= 0;

-- Directories only
SELECT * FROM snapshot WHERE byte_size = -1;

-- Live entries (not deleted)
SELECT * FROM snapshot WHERE del_time IS NULL;

-- Deleted entries (tombstones)
SELECT * FROM snapshot WHERE del_time IS NOT NULL;
```

### Walking a Directory

To list all entries in a directory:

```sql
SELECT * FROM snapshot WHERE parent_id = ? AND del_time IS NULL;
```

Pass the hash of the directory path (with trailing slash) to get its immediate children.

## Queue Table (Peer Databases Only)

Each peer database contains a queue of paths awaiting sync:

```sql
CREATE TABLE IF NOT EXISTS queue (
    path TEXT PRIMARY KEY,      -- relative path (deduplicates automatically)
    enqueued_at TEXT NOT NULL   -- ISO timestamp, for dropping oldest when full
);

CREATE INDEX IF NOT EXISTS idx_queue_enqueued ON queue(enqueued_at);
```

The queue is capped at 10,000 entries (configurable via `queue-max-size` in peers.conf). When full, the oldest entries are dropped to make room for new ones. Recent changes get the "fast path" (immediate sync when connected); older changes that overflow the queue are caught by the peer walk.

### Enqueue Logic

When enqueueing a path:
1. If path already exists: update `enqueued_at` to current time (refreshes priority)
2. If queue is full: delete oldest entry (by `enqueued_at`), then insert
3. Otherwise: insert normally

### Why Capped?

A peer offline for a month could accumulate unbounded queue entries. The cap ensures:
- Queue stays bounded regardless of disconnection duration
- Recent changes sync immediately when peer reconnects
- Peer walk (which always runs on connect) catches anything that overflowed

The queue is an optimization for fast fan-out, not the source of truth. The peer walk is the source of truth.
### Dequeue Order

Workers dequeue in FIFO order (oldest first by `enqueued_at`). The "recent-first priority" in overflow behavior means that when the queue is full, the oldest entries are dropped, ensuring recent changes are preserved. The actual processing order is oldest-first among surviving entries.

## Config Table (Peer Databases)

Each peer database contains a config table for peer-specific runtime state:

```sql
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

Currently stores:
- `last_walk_time` -- timestamp of last successful peer walk, used for deletion attribution

When the peer walker finds a file in the snapshot that no longer exists on the peer filesystem, it sets `del_time` to this value. This is a conservative estimate: the file could have been deleted any time after we last walked, so we use the oldest possible time. If a local file was modified after our last walk of the peer, the local modification wins over the peer deletion.

## Timestamps

All timestamps use the format `YYYYMMDDTHHmmss.ffffffZ` -- UTC with microsecond precision. Examples:
- `20260314T091523.847291Z`
- `20260101T000000.000000Z`

This format sorts lexicographically and is unambiguous.

Generated timestamps are monotonic within a process. If the system clock would produce the same timestamp as the last generated one, add one microsecond. This guarantees uniqueness for BACK/ directories, XFER staging, and any other use of generated timestamps.

## Tombstone Lifecycle

The same tombstone logic applies to both local and peer snapshots.

**When a file is deleted:**
1. The walker detects the deletion (file in snapshot but not on filesystem)
2. The snapshot row is updated: set `del_time` to the deletion timestamp
   - Local watcher: uses current time (precise)
   - Local walker: uses `last_walk_time` from local config table
   - Peer walker: uses `last_walk_time` from peer config table
3. The row remains in the database as a tombstone

Tombstones older than `tombstone-retention-days` (default: 180, ~6 months) are deleted during walks. Why 6 months default? Long enough for occasionally-connected peers to sync before the record expires; short enough to not grow forever.

**When a deleted file reappears (resurrection):**
1. The walker detects a file on filesystem with `del_time` set in snapshot
2. Compare file's `mod_time` to `del_time`:
   - If `mod_time > del_time`: file wins. Clear `del_time`, update `mod_time` and `byte_size`.
   - If `mod_time <= del_time`: deletion wins. The file is stale -- delete it, move to BACK/.

## Directories

Directories are stored in the snapshot table with `byte_size = -1` and `mod_time = NULL`. Directories sync like files: creation and deletion propagate to peers.

Directory entries are created automatically when:
- A directory is encountered during a filesystem walk
- A file is created (parent directories are recorded)

Directory deletion propagates to peers. When deleting a directory on a peer, the directory is only removed if it is empty. If the peer has files in that directory (not yet synced), those files sync first; the directory deletion will succeed on a subsequent pass once empty.

See `reconciliation.md` for directory-specific decision rules.

## Case Sensitivity and Character Sets

KitchenSync preserves filename case and byte sequences exactly as the filesystem reports it. No normalization is performed.

**Case sensitivity:** If a case-sensitive filesystem (Linux) has two files that differ only in case (e.g., `README.txt` and `readme.txt`), syncing to a case-insensitive filesystem (Windows) will collapse them to one file. The second file transferred overwrites the first (with the first going to BACK/). On the next peer walk, the "missing" file appears deleted, and that tombstone propagates back -- deleting one of the original files.

**Non-ASCII characters:** KitchenSync does not attempt to predict which characters each device's filesystem will accept. Non-ASCII alphanumeric characters in paths (accented letters, emoji, etc.) may cause problems when syncing across different systems.

This is accepted behavior. Workaround: stick to ASCII filenames when syncing across filesystem types. Deleted files are recoverable from BACK/.
