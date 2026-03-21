# Database

Single SQLite database at `kitchensync.db` inside the config directory (default `~/.kitchensync/`). The database path is not separately configurable. WAL mode. Foreign keys enabled.

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

CREATE TABLE IF NOT EXISTS peer (
    peer_id INTEGER PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS peer_url (
    peer_id INTEGER NOT NULL REFERENCES peer(peer_id),
    normalized_url TEXT NOT NULL UNIQUE,
    PRIMARY KEY (peer_id, normalized_url)
);

CREATE TABLE IF NOT EXISTS snapshot (
    id TEXT NOT NULL,
    peer_id INTEGER NOT NULL REFERENCES peer(peer_id),
    parent_id TEXT NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    last_seen TEXT,
    deleted_time TEXT,
    PRIMARY KEY (id, peer_id)
);
CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_last_seen ON snapshot(last_seen);
CREATE INDEX IF NOT EXISTS idx_snapshot_deleted ON snapshot(deleted_time);
```

## URL Normalization

URLs are normalized before storage and lookup:
- Lowercase the scheme and hostname
- Remove default port (22 for SFTP)
- Collapse consecutive slashes in the path
- Remove trailing slash from the path
- Bare paths (no scheme) are converted to `file://` URLs
- `file://` URLs: resolve to absolute path (from cwd)
- Percent-decode unreserved characters

Examples:
- `c:/photos/` → `file:///c:/photos`
- `./data` → `file:///home/user/data` (resolved from cwd)
- `SFTP://Host:22/path/` → `sftp://host/path`
- `sftp://host//docs/` → `sftp://host/docs`

## Peer Identity

The database stores stable peer IDs and maps URLs to them. Group structure lives entirely in the config file — the database does not track groups.

### Config file structure

The config file is JSON with `//` and `/* */` comments allowed. Comments are stripped before parsing.

```json5
{
  "peer_groups": [
    {
      "name": "photos",
      "peers": [
        { "name": "local", "urls": ["file:///c:/photos"], "canon": true },
        { "name": "nas", "urls": ["sftp://bilbo@nas/photos"] }
      ]
    },
    {
      "name": "docs",
      "peers": [
        { "name": "laptop", "urls": ["file:///c:/docs"] },
        { "name": "nas", "urls": [
            { "url": "sftp://bilbo@192.168.1.50/docs", "max-connections": 20 },
            { "url": "sftp://bilbo@nas.vpn/docs", "connection-timeout": 60 }
          ]
        }
      ]
    }
  ]
}
```

- **`peer_groups`**: list of peer groups. Each group has a `peers` list.
- **`peers`**: list of peers in the group. Each peer is a distinct location with distinct data. A peer has a `urls` list and optional `"canon": true`.
- **`urls`**: fallback URLs for one peer (same data, different network paths). Each entry is a string or an object with `"url"` plus optional per-URL settings (e.g. `"max-connections"`, `"connection-timeout"`). All URLs for a peer map to the same `peer_id`.

The `canon` flag is a boolean on the peer object — at most one peer per group may have `"canon": true`. Canon is only set by editing the config file; the CLI `!` suffix is a one-time override that is not persisted.

### Startup reconciliation

On every startup, after merging CLI arguments into the config, reconciliation runs in two passes:

#### Pass 1: Recognize (read-only)

For each peer in the config, normalize all its URLs (primary + fallbacks) and look each up in `peer_url`. Build a mapping of config peers to database peer IDs:

- If any URL matches an existing `peer_id` → this config peer maps to that `peer_id`.
- If multiple URLs match different `peer_id` values → those peers are being merged by the config. Use the lowest `peer_id`; snapshot rows from the others will be migrated in pass 2.
- If no URLs match → this is a new peer (will be created in pass 2).
- If two different config peers resolve to the same `peer_id` → config error (ambiguous peer identity).

After pass 1, every config peer is mapped to either an existing `peer_id` or marked as new.

#### Pass 2: Rewrite (writes)

1. **Create** new `peer` rows for anything marked new in pass 1.
2. **Migrate** snapshot rows when peers are being merged: update `snapshot.peer_id` from the old ID to the surviving ID. Delete the now-empty old `peer` rows.
3. **Rewrite `peer_url`**: delete all rows, re-insert from the config. This ensures the URL-to-peer mapping exactly mirrors the config file.

### Why this works

- **Rename a peer's URL**: old URL is removed from `peer_url`, new URL is added. Same `peer_id`, all snapshot history preserved.
- **Move a peer between groups**: just a config file edit. Snapshot history stays (it's keyed by `peer_id`, not by group).
- **Add a fallback URL**: new row in `peer_url` for the existing `peer_id`.
- **Split a group**: just a config file edit. Snapshot history per peer is unchanged.
- **Merge groups**: just a config file edit. Individual peer snapshot histories are preserved.

### CLI-driven group resolution

When the user specifies URLs on the command line:
1. Normalize each URL, look it up in `peer_url`.
2. If any URL matches an existing peer, load that peer's group from the config file. That is the active group for this run.
3. If URLs match peers in different groups (per the config file), that is a config error.
4. New URLs (not in the database) are added to the active group as new peers — or a new group is created if no URLs matched.
5. The updated group is written back to the config file.

## Snapshot

Tracks per-peer state — one row per path per peer that has (or had) the entry.

- **id**: xxHash64 of full relative path (forward slashes), base62-encoded (11 characters)
- **peer_id**: stable integer, foreign key to `peer` table
- **parent_id**: xxHash64 of parent path with trailing `/`, base62-encoded. Root entries use the hash of `/`.
- **basename**: final path component
- **mod_time**: `YYYY-MM-DD_HH-mm-ss_ffffffZ` — the entry's mod_time as last observed on this peer
- **byte_size**: bytes for files, -1 for directories
- **last_seen**: `YYYY-MM-DD_HH-mm-ss_ffffffZ` or NULL — set to the current sync timestamp when the entry is confirmed present on this peer (via listing or after a completed copy). NULL when a push has been decided but the copy has not yet completed. Only confirmed presence updates this field.
- **deleted_time**: `YYYY-MM-DD_HH-mm-ss_ffffffZ` or NULL — NULL while the entry exists (or a copy is pending). Set when the entry is confirmed absent on this peer. The value is copied from `last_seen` at the time of detection (a conservative estimate — the real deletion happened sometime after `last_seen`).

Updated during traversal, before file copies complete, except for `last_seen` on copy destinations — that is set after the copy completes. If copies don't finish, the destination row has `deleted_time = NULL` and `last_seen` unchanged (NULL for first-time targets). The next run applies rule 4b: since `last_seen` is NULL or old, it does not exceed the source's mod_time, so the copy is re-enqueued.

## Tombstones

When a file is confirmed absent on a peer where a snapshot row exists with `deleted_time = NULL`, the row is retained and `deleted_time` is set to the current value of `last_seen` (a conservative estimate — the real deletion happened sometime after that). A row with `deleted_time IS NOT NULL` is a tombstone. Tombstones are purged when `deleted_time` is older than `tombstone-retention-days` (default: 180).

## Path Hashing

Paths are hashed with xxHash64 (seed 0) and encoded as base62 (digits `0-9`, uppercase `A-Z`, lowercase `a-z`). 64 bits → 11 characters, zero-padded.

- Forward slashes, no leading slash
- Trailing slash for directories and parent paths
- `docs/readme.txt` → hash of `docs/readme.txt`
- `docs/notes/` (dir) → hash of `docs/notes/`
- Parent of `docs/readme.txt` → hash of `docs/`
- Parent of root entries → hash of `/`
- The sync root directory itself is not tracked in the snapshot — only its children are. Traversal begins by listing the root; the root has no snapshot row.

## Timestamps

Format: `YYYY-MM-DD_HH-mm-ss_ffffffZ` — UTC, microsecond precision, lexicographic sort, filesystem-safe. This format is used everywhere timestamps appear: database columns, BACK/ directory names, XFER/ directory names, and log entries.

Monotonic within a process: add 1μs on collision.
