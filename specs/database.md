# Database

Each peer stores its own snapshot in `{peer-root}/.kitchensync/snapshot.db`. SQLite, WAL mode, foreign keys enabled.

At the start of a run, each peer's `snapshot.db` is downloaded to a local temporary directory (`{tmp}/{uuid}/snapshot.db`). All reads and writes happen against this local copy. After sync completes, the updated database is written back using TMP staging (see algorithm.md). If a peer has no existing `snapshot.db`, a new one is created locally.

Concurrent runs are not coordinated. If two runs overlap, the last snapshot upload wins. Decisions from the losing run are re-discovered on the next run — correctness is preserved, but some work is repeated.

## Schema

```sql
CREATE TABLE snapshot (
    id           TEXT PRIMARY KEY,  -- xxHash64 of relative path, base62-encoded (11 chars)
    parent_id    TEXT NOT NULL,     -- xxHash64 of parent dir's relative path, base62-encoded
    basename     TEXT NOT NULL,     -- final path component
    mod_time     TEXT NOT NULL,     -- YYYY-MM-DD_HH-mm-ss_ffffffZ
    byte_size    INTEGER NOT NULL,  -- bytes for files, -1 for directories
    last_seen    TEXT,              -- YYYY-MM-DD_HH-mm-ss_ffffffZ or NULL
    deleted_time TEXT,              -- YYYY-MM-DD_HH-mm-ss_ffffffZ or NULL
    FOREIGN KEY (parent_id) REFERENCES snapshot(id)
);

CREATE INDEX idx_parent_id ON snapshot(parent_id);
CREATE INDEX idx_last_seen ON snapshot(last_seen);
CREATE INDEX idx_deleted_time ON snapshot(deleted_time);

PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
```

## Timestamps

Format: `YYYY-MM-DD_HH-mm-ss_ffffffZ` — UTC, microsecond precision, lexicographic sort, filesystem-safe.

Example: `2026-03-30_17-45-48_000000Z`

This format is used everywhere timestamps appear: database columns, BAK/ directory names, TMP/ directory names, and log output. Note the separators: hyphens in the date, hyphens in the time (not colons), underscore between date and time, underscore before microseconds, trailing `Z`.

Monotonic within a process: add 1us on collision. The monotonic timestamp generator is process-global — a single generator is used for all BAK directory names, TMP directory names, and database timestamps, ensuring no collisions across concurrent operations.

## Path Hashing

Paths are hashed with xxHash64 (seed 0) and encoded as base62 (digits `0-9`, uppercase `A-Z`, lowercase `a-z`). 64 bits -> 11 characters, zero-padded. Alphabet: `0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz` (0=`0`, 9=`9`, 10=`A`, 35=`Z`, 36=`a`, 61=`z`). Most-significant digit first, zero-padded to 11 characters.

Rules:
- Forward slashes, no leading slash, no trailing slash
- Files and directories hashed identically (`byte_size = -1` distinguishes directories)
- `docs/readme.txt` -> hash of `docs/readme.txt`
- `docs/notes` (dir) -> hash of `docs/notes`
- Parent of `docs/readme.txt` -> hash of `docs`
- Parent of root entries -> hash of `/` (sentinel)
- The sync root directory itself is not tracked — only its children. Traversal begins by listing the root; the root has no snapshot row.
- A sentinel row must be inserted when creating a new snapshot database so that root-level entries satisfy the foreign key on `parent_id`. The sentinel's `parent_id` references itself:
  ```sql
  INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size)
  VALUES ('<hash-of-/>', '<hash-of-/>', '', '0000-00-00_00-00-00_000000Z', -1);
  ```

## URL Normalization

URLs are normalized before any comparison, lookup, or connection attempt:
- Lowercase the scheme and hostname
- Remove default port (22 for SFTP)
- Collapse consecutive slashes in the path
- Remove trailing slash from the path
- Bare paths (no scheme) are converted to `file://` URLs
- `file://` URLs: resolve to absolute path (from cwd)
- Percent-decode unreserved characters
- Strip query-string parameters (`?mc=5` etc. are not part of identity)

Examples:
- `c:/photos/` -> `file:///c:/photos`
- `./data` -> `file:///home/user/data` (resolved from cwd)
- `SFTP://Host:22/path/` -> `sftp://host/path`
- `sftp://host//docs/` -> `sftp://host/docs`
- `sftp://host/path?mc=5` -> `sftp://host/path`

Normalization is applied before any connection attempt — not just before storage.

### OS-Native Paths for file:// URLs

On Windows, the normalized URL path has a leading slash (`/c:/photos`), but OS filesystem calls require the native format (`c:/photos`). Provide a separate accessor that strips the leading slash on Windows drive-letter paths. Use this accessor for all filesystem operations (MkdirAll, Open, Stat, etc.) on `file://` URLs.

## Tombstones

When an entry is confirmed absent on a peer where a snapshot row exists with `deleted_time = NULL`, the row is retained and `deleted_time` is set to the current value of `last_seen`. A row with `deleted_time IS NOT NULL` is a tombstone. Tombstones are purged when `deleted_time` is older than `--td` days (default: 180).
