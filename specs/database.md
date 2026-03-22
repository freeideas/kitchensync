# Database

Each peer stores its own snapshot in `{peer-root}/.kitchensync/snapshot.db`. SQLite, WAL mode, foreign keys enabled.

At the start of a run, each peer's `snapshot.db` is downloaded to a local temporary directory (`{tmp}/{uuid}/snapshot.db`). All reads and writes happen against this local copy. After sync completes, the updated database is written back atomically: upload as `snapshot-new.db`, then rename to `snapshot.db`. If a peer has no existing `snapshot.db`, a new one is created locally.

## Schema

### Snapshot

Tracks what this peer had (or has had) — one row per path.

| Column       | Type    | Notes                                                                                                  |
| ------------ | ------- | ------------------------------------------------------------------------------------------------------ |
| id           | TEXT    | Primary key. xxHash64 of full relative path, base62-encoded (11 chars)                                 |
| parent_id    | TEXT    | xxHash64 of parent directory's relative path, base62-encoded. Root entries use hash of `/` as sentinel  |
| basename     | TEXT    | Final path component, not null                                                                         |
| mod_time     | TEXT    | `YYYY-MM-DD_HH-mm-ss_ffffffZ` — entry's mod_time as last observed on this peer, not null. For directories, recorded but not used in decisions (see multi-tree-sync.md, Directory Decisions) |
| byte_size    | INTEGER | Bytes for files, -1 for directories, not null                                                          |
| last_seen    | TEXT    | `YYYY-MM-DD_HH-mm-ss_ffffffZ` or NULL — set when entry is confirmed present (via listing or completed copy). NULL when a copy has been decided but not yet completed |
| deleted_time | TEXT    | `YYYY-MM-DD_HH-mm-ss_ffffffZ` or NULL — NULL while entry exists. Set to `last_seen` value when entry is confirmed absent |

Indexes on `parent_id`, `last_seen`, and `deleted_time`.

Updated during traversal, before file copies complete, except for `last_seen` on copy destinations — that is set after the copy completes. If copies don't finish, the destination row has `deleted_time = NULL` and `last_seen` unchanged (NULL for first-time targets). The next run applies rule 4b: since `last_seen` is NULL or old, it does not exceed the source's mod_time, so the copy is re-enqueued.

## URL Normalization

URLs are normalized before any comparison or lookup:
- Lowercase the scheme and hostname
- Remove default port (22 for SFTP)
- Collapse consecutive slashes in the path
- Remove trailing slash from the path
- Bare paths (no scheme) are converted to `file://` URLs
- `file://` URLs: resolve to absolute path (from cwd)
- Percent-decode unreserved characters
- Strip query-string parameters (per-URL settings like `?mc=5` are not part of the identity)

Examples:
- `c:/photos/` → `file:///c:/photos`
- `./data` → `file:///home/user/data` (resolved from cwd)
- `SFTP://Host:22/path/` → `sftp://host/path`
- `sftp://host//docs/` → `sftp://host/docs`
- `sftp://host/path?mc=5` → `sftp://host/path`

## Tombstones

When an entry is confirmed absent on a peer where a snapshot row exists with `deleted_time = NULL`, the row is retained and `deleted_time` is set to the current value of `last_seen` (a conservative estimate — the real deletion happened sometime after that). A row with `deleted_time IS NOT NULL` is a tombstone. Tombstones are purged when `deleted_time` is older than `--td` (tombstone retention days, default: 180).

## Path Hashing

Paths are hashed with xxHash64 (seed 0) and encoded as base62 (digits `0-9`, uppercase `A-Z`, lowercase `a-z`). 64 bits → 11 characters, zero-padded.

- Forward slashes, no leading slash, no trailing slash (files and directories are hashed identically; `byte_size = -1` distinguishes directories)
- `docs/readme.txt` → hash of `docs/readme.txt`
- `docs/notes` (dir) → hash of `docs/notes`
- Parent of `docs/readme.txt` → hash of `docs`
- Parent of `docs/notes` → hash of `docs`
- Parent of root entries → hash of `/` (sentinel)
- The sync root directory itself is not tracked in the snapshot — only its children are. Traversal begins by listing the root; the root has no snapshot row.

## Timestamps

Format: `YYYY-MM-DD_HH-mm-ss_ffffffZ` — UTC, microsecond precision, lexicographic sort, filesystem-safe. This format is used everywhere timestamps appear: database columns, BAK/ directory names, TMP/ directory names, and log output.

Monotonic within a process: add 1μs on collision.
