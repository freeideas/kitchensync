# Per-peer snapshot database — schema, path hashing, timestamp formatting, and tombstone cascade.

## Purpose
Every peer carries a local SQLite snapshot at `.kitchensync/snapshot.db` that records what that peer had, or has had, at every path. The snapshot database component owns the schema, the path-hash identity scheme, the timestamp string format, and the tombstone/cascade mechanics for a single such file. It is a pure local-storage component: open a database file, read and write rows, hash paths, format timestamps. It does no networking, no listing, no copying, no decision-making — those are the orchestrator's concerns. It is consulted before traversal acts and updated as decisions are made.

## API surface

The component exposes four groups of operations, all scoped to a single open snapshot database handle.

### Lifecycle

- **Open** a snapshot database at a given local filesystem path. If the file does not exist, create it with the schema below. If it exists, verify the schema and open it for reading and writing. Returns a handle. The file is opened with WAL journal mode and foreign keys enabled.
- **Close** a handle, flushing any pending writes.

### Identity and timestamp helpers

- **Hash a path** — given a relative path string (forward slashes, no leading or trailing slash), return an 11-character base62 identifier: xxHash64 with seed 0, encoded with digits `0-9`, uppercase `A-Z`, lowercase `a-z`, zero-padded to 11 characters. Files and directories hash identically — the `byte_size` field distinguishes them.
- **Root parent sentinel** — the parent-id value used for entries directly under the sync root, defined as the hash of the literal string `/`.
- **Current timestamp** — return the current UTC time formatted as `YYYY-MM-DD_HH-mm-ss_ffffffZ` (microsecond precision, filesystem-safe, lexicographically sortable). Within a single open handle, the returned value is strictly monotonic: if a caller requests two timestamps and the clock has not advanced, the second value is one microsecond after the first.

### Row operations

A snapshot row carries: `id`, `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, `deleted_time`. `byte_size` is bytes for files and `-1` for directories. `last_seen` and `deleted_time` may be NULL. Timestamps in stored rows use the format above. A row with `deleted_time IS NOT NULL` is a tombstone.

- **Lookup row by id** — return the row for a given id, or nothing if no row exists.
- **List child rows** — return all rows whose `parent_id` matches a given id. The caller uses this to learn which paths the snapshot has under a directory.
- **Upsert a confirmed-present row** — given relative path, basename, mod_time, byte_size, and a confirmation timestamp, write or replace the row so that the recorded mod_time and byte_size match the supplied values, `last_seen` is set to the supplied confirmation timestamp, and `deleted_time` is cleared. Used when traversal observes an entry on the peer, or when a copy completes on the peer.
- **Upsert a decided-but-unconfirmed row** — given relative path, basename, mod_time, byte_size, write or replace the row with the supplied mod_time and byte_size and `deleted_time` cleared, **without** modifying `last_seen` (preserve any existing value; leave NULL if no prior row). Used when the peer is decided as a copy destination, before the copy has completed.
- **Mark a copy completed** — set `last_seen` on the row for a given path to the supplied confirmation timestamp.
- **Mark an entry absent** — given the path of an entry confirmed absent on the peer: if a row exists with `deleted_time IS NULL`, set `deleted_time` to that row's current `last_seen` value and leave `last_seen` unchanged. If `deleted_time` is already set, do nothing (idempotent). If no row exists, do nothing.
- **Cascade-tombstone a subtree** — given the id of a displaced directory and a tombstone timestamp, set `deleted_time` to the supplied timestamp for every descendant row reachable through `parent_id` links from that id whose `deleted_time IS NULL`. The implementation uses a recursive walk down the `parent_id` graph so that only true descendants of the displaced entry are affected — unrelated rows whose ancestor happens to be tombstoned are not touched.

### Purge

- **Purge stale rows** — given a cutoff timestamp, delete rows that satisfy either of:
  - `deleted_time IS NOT NULL AND deleted_time < cutoff` (expired tombstones), or
  - `deleted_time IS NULL AND (last_seen IS NULL OR last_seen < cutoff)` (orphaned rows from entries that disappeared without being visited).

  Used at the start of a run; the orchestrator chooses the cutoff from its retention setting.

### Schema (informational)

The single table is named `snapshot`, with columns and types matching the table in `database.md` §"Schema". Indexes exist on `parent_id`, `last_seen`, and `deleted_time`. The schema is created on first open and is not user-configurable.

## Anchoring

- **SQLite** (external standard) — file format, WAL journal mode, foreign-key enforcement, recursive CTE for the cascade walk.
- **xxHash64** (external standard, seed 0) — path hashing function.
- **Base62 with digits `0-9`, uppercase `A-Z`, lowercase `a-z`** — well-known alphanumeric encoding for the 11-character path identifiers, defined fully in `database.md` §"Path Hashing".
- **UTC, microsecond precision** — the timestamp format `YYYY-MM-DD_HH-mm-ss_ffffffZ` is defined in `database.md` §"Timestamps".
- **`database.md` §"Schema"** — column list, types, NULL rules, index list.
- **`database.md` §"Path Hashing"** — hash function, encoding, root sentinel, leading/trailing slash rules.
- **`database.md` §"Timestamps"** — format and monotonic-within-process rule.
- **`database.md` §"Tombstones"** — when a row becomes a tombstone and idempotency on repeated absence.
- **`multi-tree-sync.md` §"Snapshot Updates"** — the row-update operations the orchestrator invokes during traversal, including the cascade SQL.
- **`multi-tree-sync.md` §"Orphaned Snapshot Rows"** — the purge rules for stale and tombstoned rows.
