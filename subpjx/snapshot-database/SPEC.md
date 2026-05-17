# Snapshot Database

A Java 21 library for storing and updating the snapshot history of one file
tree in a local SQLite database. It owns the SQLite schema, path identifiers,
timestamp text format, row lookup, presence updates, tombstones, stale-row
purge, and recursive descendant tombstone cascades.

The library is for local snapshot state only. It does not copy files, list
directories, parse command lines or URLs, normalize peer URLs, open network
connections, decide conflict winners, apply ignore rules, create BAK or TMP
filesystem paths, upload or download databases, schedule work, or log
diagnostics. Callers provide relative paths, observed file metadata, and current
timestamps, then execute any filesystem operations outside this library.

## Public API

The API may use normal Java classes, records, interfaces, or equivalent types,
but it must expose this behavior.

### Data Shapes

`SnapshotTime`

A UTC timestamp stored as text in this exact filesystem-safe format:

```text
YYYY-MM-DD_HH-mm-ss_ffffffZ
```

The fractional field is exactly six decimal digits. Lexicographic order matches
time order. Invalid timestamp text is rejected.

`SnapshotTimestampGenerator`

Generates `SnapshotTime` values. Each call returns a value strictly greater
than every value previously returned by that generator instance. If the wall
clock has not advanced, the generator adds one microsecond.

`EntryKind`

| Value | Meaning |
| --- | --- |
| `file` | A regular file. |
| `directory` | A directory. |

`EntryMetadata`

| Field | Meaning |
| --- | --- |
| `kind` | `file` or `directory`. |
| `mod_time` | Last observed modification time as `SnapshotTime`. |
| `byte_size` | File size in bytes for files, or `-1` for directories. |

File `byte_size` must be zero or greater. Directory `byte_size` must be `-1`.
Directory `mod_time` is recorded but not interpreted by this library.

`SnapshotRow`

| Field | Meaning |
| --- | --- |
| `id` | Path ID: xxHash64 of the normalized relative path, base62-encoded to 11 characters. |
| `parent_id` | Path ID of the parent relative path, or the root sentinel for root children. |
| `relative_path` | Slash-separated path label reconstructed from the caller request or row context. |
| `basename` | Final path component. |
| `kind` | `directory` when `byte_size = -1`; otherwise `file`. |
| `mod_time` | Stored modification time. |
| `byte_size` | File size in bytes, or `-1` for directories. |
| `last_seen` | Timestamp when the entry was last confirmed present, or absent. |
| `deleted_time` | Tombstone deletion estimate, or absent while the entry is not tombstoned. |

`PathId`

Path IDs are generated as follows:

- Normalize relative paths with forward slashes, no leading slash, no trailing
  slash, and no empty path segments.
- The sync root directory itself is not tracked and has no row.
- Root children use the xxHash64/base62 ID of `/` as their `parent_id`.
- Hash bytes are the UTF-8 bytes of the normalized relative path.
- Hash algorithm is xxHash64 with seed `0`.
- Encoding is base62 with alphabet `0-9`, `A-Z`, `a-z`.
- Encoded output is left-padded with zeroes to exactly 11 characters.

Examples:

| Input path | ID |
| --- | --- |
| `/` root sentinel | `JyBskcNRrBK` |
| `docs` | `H41WPg3SlMv` |
| `docs/readme.txt` | `K5EzsWuLZ04` |

### SQLite Schema

The database contains exactly one application table named `snapshot`
(singular, lowercase). The library must not create a pluralized synonym, a
view, or alternate table spelling.

```sql
CREATE TABLE snapshot (
    id TEXT PRIMARY KEY,
    parent_id TEXT NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    last_seen TEXT,
    deleted_time TEXT
);

CREATE INDEX snapshot_parent_id_idx ON snapshot(parent_id);
CREATE INDEX snapshot_last_seen_idx ON snapshot(last_seen);
CREATE INDEX snapshot_deleted_time_idx ON snapshot(deleted_time);
```

Each opened connection enables SQLite foreign keys and uses rollback-journal
mode, not WAL mode. SQLite sidecar files are not part of the snapshot state.

### Operations

`SnapshotDatabase.open(db_file) -> SnapshotDatabase`

Opens a local SQLite database file and initializes the schema if the file is
empty. The operation creates the database file when it does not exist. It does
not create parent directories for `db_file`.

`SnapshotDatabase.close()`

Closes the database. Closing is idempotent.

`SnapshotDatabase.has_rows() -> boolean`

Returns `true` when the `snapshot` table contains at least one row.

`SnapshotDatabase.path_id(relative_path) -> PathId`

Returns the deterministic path ID for a normalized relative path. The root
sentinel is available as `SnapshotDatabase.root_parent_id()`.

`SnapshotDatabase.lookup(relative_path) -> Optional<SnapshotRow>`

Returns the row for one path, or absent when no row exists.

`SnapshotDatabase.record_present(relative_path, metadata, seen_at)`

Upserts a row for an entry confirmed present by a live listing or completed
directory creation. The row stores `metadata.mod_time`, `metadata.byte_size`,
sets `last_seen = seen_at`, and clears `deleted_time`.

`SnapshotDatabase.record_copy_pending(relative_path, metadata)`

Upserts a row for a file copy that has been decided but has not completed. The
row stores the winning file metadata and clears `deleted_time`. It does not
update `last_seen`; if no row exists, `last_seen` remains absent.

`SnapshotDatabase.confirm_copy_completed(relative_path, seen_at)`

Sets `last_seen = seen_at` for an existing row after a pending file copy
finishes successfully. If no row exists, the operation returns `not_found`.

`SnapshotDatabase.mark_absent(relative_path)`

Records an observed absence. If the row exists and `deleted_time` is absent, set
`deleted_time` to the row's current `last_seen` and do not update `last_seen`.
If the row is already tombstoned or no row exists, the operation succeeds
without changing any row.

`SnapshotDatabase.mark_displaced(relative_path)`

Records an entry moved aside by the caller. If no row exists, the operation
succeeds without changing any row. If the row exists, the deletion estimate is
the row's existing `deleted_time` when already tombstoned, otherwise the row's
current `last_seen`. The operation sets `deleted_time` to that estimate for the
target row and every descendant row whose `deleted_time` is absent.

The cascade is scoped to one database and uses parent-child path IDs. It must
not update unrelated rows that merely share a basename or have an already
tombstoned ancestor outside the displaced subtree. The update is one
transaction.

The cascade behavior is equivalent to:

```sql
WITH RECURSIVE subtree(id) AS (
    VALUES(?displaced_id)
    UNION ALL
    SELECT s.id FROM snapshot s
    JOIN subtree st ON s.parent_id = st.id
    WHERE s.deleted_time IS NULL
)
UPDATE snapshot
SET deleted_time = ?deleted_time
WHERE deleted_time IS NULL
AND id IN (SELECT id FROM subtree);
```

`SnapshotDatabase.purge(cutoff_time) -> PurgeResult`

Deletes stale rows:

- rows where `deleted_time` is present and older than `cutoff_time`;
- rows where `deleted_time` is absent and `last_seen` is older than
  `cutoff_time`;
- rows where both `deleted_time` and `last_seen` are absent.

`PurgeResult` reports the number of rows deleted.

## Observable Behavior

- All write operations are transactional. A failed write leaves the database in
  its previous committed state.
- Path IDs are stable across operating systems and process runs.
- The root directory itself is never inserted as a row.
- `parent_id` is derived from the normalized parent path, not from database
  lookup state.
- `record_present` clears tombstones when a path reappears.
- `record_copy_pending` preserves any previous `last_seen` value so an
  interrupted copy can be recognized by the caller on a later run.
- `mark_absent` is idempotent and preserves an existing tombstone estimate.
- `mark_displaced` applies one deletion estimate to the whole displaced
  subtree.
- Purging orphaned descendants does not require all intermediate parent rows to
  still exist; stale orphan rows are removed by their own `last_seen` or absent
  `last_seen` state.
- Public operations do not write to stdout or stderr.

## Error Behavior

Operations fail with one of these categories and no partial public result:

| Category | Meaning |
| --- | --- |
| `invalid_path` | Relative path is empty, starts with `/`, ends with `/`, contains an empty segment, contains a NUL byte, or is the root directory itself. |
| `invalid_timestamp` | Timestamp text does not match `YYYY-MM-DD_HH-mm-ss_ffffffZ` or is not a valid UTC time. |
| `invalid_metadata` | File size is negative, directory size is not `-1`, or required metadata is absent. |
| `not_found` | `confirm_copy_completed` was called for a path with no row. |
| `database_error` | SQLite open, schema, query, transaction, or disk I/O failed. |

The library does not throw transport-specific, network-specific, filesystem
copy, URL parsing, or sync-decision errors because it performs none of those
operations.

## Examples

### Confirm A Listed File

Input:

```text
db = SnapshotDatabase.open("/tmp/example/snapshot.db")
record_present(
  relative_path = "docs/readme.txt",
  metadata = file mod_time=2026-05-15_10-00-00_000000Z byte_size=12,
  seen_at = 2026-05-15_10-00-05_000000Z
)
lookup("docs/readme.txt")
```

Output:

```text
SnapshotRow(
  id = "K5EzsWuLZ04",
  parent_id = "H41WPg3SlMv",
  relative_path = "docs/readme.txt",
  basename = "readme.txt",
  kind = file,
  mod_time = 2026-05-15_10-00-00_000000Z,
  byte_size = 12,
  last_seen = 2026-05-15_10-00-05_000000Z,
  deleted_time = absent
)
```

### Pending Copy Then Completion

Input:

```text
record_copy_pending(
  relative_path = "pending.bin",
  metadata = file mod_time=2026-05-15_11-00-00_000000Z byte_size=5
)
lookup("pending.bin")
confirm_copy_completed("pending.bin", 2026-05-15_11-00-09_000000Z)
lookup("pending.bin")
```

Output after `record_copy_pending`:

```text
SnapshotRow(
  id = "IdWzugtOkpp",
  parent_id = "JyBskcNRrBK",
  basename = "pending.bin",
  kind = file,
  mod_time = 2026-05-15_11-00-00_000000Z,
  byte_size = 5,
  last_seen = absent,
  deleted_time = absent
)
```

Output after `confirm_copy_completed`:

```text
last_seen = 2026-05-15_11-00-09_000000Z
deleted_time = absent
```

### Displace A Directory Subtree

Initial rows:

```text
album                id=1gmLoxZfNDN parent_id=JyBskcNRrBK last_seen=2026-05-15_09-00-00_000000Z deleted_time=absent
album/raw            id=HhJji0AtfjA parent_id=1gmLoxZfNDN last_seen=2026-05-15_09-00-01_000000Z deleted_time=absent
album/raw/a.jpg      id=00mExMtVcpq parent_id=HhJji0AtfjA last_seen=2026-05-15_09-00-02_000000Z deleted_time=absent
old.txt              id=LOHJbwgGxuj parent_id=JyBskcNRrBK last_seen=2026-05-15_08-00-00_000000Z deleted_time=absent
```

Input:

```text
mark_displaced("album")
```

Output:

```text
album                deleted_time=2026-05-15_09-00-00_000000Z
album/raw            deleted_time=2026-05-15_09-00-00_000000Z
album/raw/a.jpg      deleted_time=2026-05-15_09-00-00_000000Z
old.txt              deleted_time=absent
```

## Testing Requirements

Tests are black-box tests of the public API using temporary local SQLite files.
No external service account, SFTP server, SSH key, network access, or local
filesystem tree fixture is required. The SFTP service account used by transport
tests is not used for this component.

Do not assert SQLite `foreign_keys` by opening a separate inspector connection:
that pragma is connection-local, and this schema has no foreign-key constraints
to exercise through the public API. The implementation must still enable
foreign keys on each connection it opens.

Required scenarios:

- Opening a missing database creates the exact `snapshot` table and required
  indexes, with rollback-journal mode.
- Path IDs match xxHash64 seed `0`, the specified base62 alphabet, 11-character
  zero padding, parent IDs, and the root sentinel.
- Invalid paths, invalid timestamps, and invalid metadata return the specified
  errors without writing rows.
- `record_present` inserts a row, updates an existing row, clears
  `deleted_time`, and sets `last_seen`.
- Directory rows use `byte_size = -1`; file rows reject negative byte sizes.
- `record_copy_pending` inserts and updates winning file metadata while leaving
  `last_seen` unchanged.
- `confirm_copy_completed` updates only `last_seen` and returns `not_found` for
  a missing row.
- `mark_absent` sets `deleted_time` from `last_seen`, preserves an existing
  tombstone, and is idempotent.
- `mark_displaced` cascades one deletion estimate to descendants in the same
  database and does not affect unrelated rows.
- `purge` removes old tombstones, old live rows, and rows with absent
  `last_seen`, while preserving newer rows.
- `SnapshotTimestampGenerator` returns exact-format, strictly increasing
  timestamp text across consecutive calls through the public wrapper. Do not
  require black-box wrapper tests to force a repeated wall clock unless a clock
  injection hook is part of the wrapper surface.
- Failed transactions leave the previous committed rows observable.
- No public operation emits stdout or stderr.

Scenarios to avoid:

- Do not test network access, SFTP authentication, host-key verification, or
  connection pooling.
- Do not test command-line parsing, URL normalization, peer roles, fallback URL
  selection, or startup reachability.
- Do not test sync conflict decisions, traversal order, directory listing
  concurrency, file-copy scheduling, or transfer pipelines.
- Do not test physical BAK/TMP path creation, file renames, atomic swaps, or
  cleanup of staged filesystem directories.
- Do not test ignore-pattern parsing or `.syncignore` resolution.
- Do not require wall-clock sleeps tighter than one microsecond.

## Semantic Anchors

This specification is anchored in:

- SQLite rollback-journal databases and foreign-key connection settings
- xxHash64 with seed `0`
- The semantic source sections for snapshot storage, path hashing, timestamp
  format and monotonicity, tombstones, snapshot updates, orphaned rows, and
  descendant deletion cascades
