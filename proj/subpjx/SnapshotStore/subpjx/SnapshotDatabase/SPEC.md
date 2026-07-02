# SnapshotDatabase:

## Purpose

SnapshotDatabase owns the local SQLite `snapshot.db` file used for one peer
during a KitchenSync run. It creates empty snapshot databases, opens downloaded
temporary snapshots, enforces the exact `snapshot` schema, mutates rows for
listing, copy, directory creation, absence, displacement, and cleanup events,
and closes the file so peer upload can read a self-contained database.

This child does not exchange files with peers and does not generate path IDs or
timestamps. Callers provide the local temporary database path, row identity
values, basename values, observed file facts, and already-generated timestamp
strings. SnapshotDatabase is responsible for storing those values under the
required SQLite shape and row rules.

## Responsibilities

SnapshotDatabase exposes an operation to create a new empty local
`snapshot.db` at the caller's temporary path. It also exposes an operation to
open an existing local temporary `snapshot.db` that was downloaded from a peer.
Every database this child creates or updates must use SQLite rollback-journal
mode, not WAL mode, so the local file can later be uploaded without SQLite
sidecar files.

Each opened database is the peer's only snapshot read and write target during
the run. SnapshotDatabase reads and writes the local temporary file supplied by
the caller; it does not read from or write to the peer-side
`.kitchensync/snapshot.db` path.

SnapshotDatabase creates and preserves exactly one SQLite table named
`snapshot`, with no views and no additional tables. The table has exactly these
columns:

- `id TEXT PRIMARY KEY`
- `parent_id TEXT`
- `basename TEXT NOT NULL`
- `mod_time TEXT NOT NULL`
- `byte_size INTEGER NOT NULL`
- `last_seen TEXT NULL`
- `deleted_time TEXT NULL`

The table has indexes on `parent_id`, `last_seen`, and `deleted_time`.
SnapshotDatabase must not add other columns, alternate table names, or views.
If it opens a local file whose schema does not satisfy these rules, it must
report an error instead of silently adapting that file.

SnapshotDatabase exposes lookup and mutation operations for snapshot rows. The
caller supplies row identity data for each path below the sync root: `id`,
`parent_id`, and `basename`. SnapshotDatabase stores no row for the sync root
directory itself. It rejects row mutations for an empty/root path and rejects
row data whose basename is missing, because every row must represent a tracked
path below the sync root and `basename` must be the final path component.

For file rows, `byte_size` stores the file size in bytes. For directory rows,
`byte_size` stores `-1`. `mod_time` stores the entry modification time last
observed on that peer. `last_seen` is nullable: a non-NULL value means the
entry was confirmed present by listing, completed file copy, or completed
directory creation, and NULL means a copy destination has been decided but the
copy has not completed. `deleted_time` is nullable: NULL means the entry
exists, and a non-NULL value marks a tombstone. Tombstone rows remain in the
table until cleanup removes them.

SnapshotDatabase exposes these row update operations:

- Confirm present: upsert the row with the supplied `id`, `parent_id`,
  `basename`, observed `mod_time`, observed `byte_size`, supplied new
  `last_seen`, and `deleted_time = NULL`.
- Confirm absent: if the row exists and `deleted_time` is NULL, set
  `deleted_time` to that row's current `last_seen` and leave `last_seen`
  unchanged. If the row is already a tombstone or does not exist, leave it
  unchanged.
- Record intended file copy: upsert the destination file row with the supplied
  winning `mod_time`, winning `byte_size`, and `deleted_time = NULL`, while
  preserving the existing `last_seen`. If no destination row exists, insert the
  row with `last_seen = NULL`.
- Complete file copy: after the queued copy succeeds, set the destination file
  row's `last_seen` to the supplied new timestamp.
- Complete directory creation: after directory creation succeeds, upsert the
  destination directory row with the supplied directory `mod_time`,
  `byte_size = -1`, `deleted_time = NULL`, and supplied new `last_seen`.
- Complete displacement: after the peer successfully moves an entry to `BAK/`,
  set that entry row's `deleted_time` to the row's previous `last_seen` and
  leave `last_seen` unchanged.

Failed directory creation and failed displacement must not call these mutation
operations. If a caller recorded an intended file copy and the app exits before
that copy finishes, the destination row remains with `deleted_time = NULL` and
its previous `last_seen`, or `last_seen = NULL` for a first-time destination.

SnapshotDatabase exposes a completed directory displacement cascade operation.
The operation first uses the displaced directory row's previous `last_seen` as
the deletion estimate, then writes that value as `deleted_time` on every
non-tombstone row reachable from the displaced directory by following
`parent_id` links in the same local database. The cascade includes the
displaced directory row, does not change already tombstoned rows, does not
change rows outside the displaced subtree, and never touches another peer's
database.

SnapshotDatabase exposes opportunistic cleanup for old rows. Cleanup removes
tombstone rows whose `deleted_time` is older than the caller's
`--keep-del-days` cutoff. Cleanup also removes obsolete non-tombstone orphan
rows that cannot be reached by a directory displacement cascade after their
`last_seen` is older than the same cutoff. Cleanup is maintenance work: callers
must be able to make sync decisions without waiting for cleanup to finish in
the current run.

SnapshotDatabase exposes a prepare-for-upload operation for a local temporary
`snapshot.db`. Before returning success, it commits or rolls back every open
transaction against that file, finalizes every statement, cursor, and reader,
and closes every SQLite connection it owns for that file. After success,
transport upload can read the closed local file directly, and the database must
be usable as a self-contained SQLite database without WAL, SHM, journal, or
other SQLite sidecar files.

Errors from SQLite open, schema validation, schema creation, row mutation,
cleanup, transaction completion, statement finalization, or connection close
are reported to the caller. A failed mutation must not be reported as success.
Operations that are specified as no-ops, such as confirming absence for an
already tombstoned row, return success without changing the row.

## Boundaries

SnapshotDatabase does not recover, download, upload, rename, delete, or close
peer-side files. Peer-side `.kitchensync/snapshot.db` handling and snapshot
SWAP staging belong to the peer file exchange child.

SnapshotDatabase does not compute `id` or `parent_id`, normalize paths, choose
which peer owns a decision, generate timestamps, format log output, parse
command-line options, or decide `--keep-del-days`. It stores the identity
values, basename values, observed file facts, and timestamp strings supplied by
callers.

SnapshotDatabase does not copy user files, create peer directories, displace
entries to `BAK/`, set filesystem modification times, or decide whether those
actions should happen. It only records successful or intended outcomes when the
caller invokes the matching row operation.

SnapshotDatabase does not block sync decisions on cleanup. It provides cleanup
as a database maintenance operation whose caller may run independently from the
decision path.

## Invariants

- Every database created or updated by this child uses rollback-journal mode.
- During a run, all snapshot reads and writes use the peer's local temporary
  `snapshot.db` copy.
- The schema is exactly one `snapshot` table, no views, the seven specified
  columns, and indexes on `parent_id`, `last_seen`, and `deleted_time`.
- Every stored row represents a tracked path below the sync root.
- The sync root directory itself has no row.
- Tombstones remain in the `snapshot` table until cleanup removes them.
- Newly created tombstones copy the row's existing `last_seen` value into
  `deleted_time`; they do not use a new generated timestamp.
- Directory displacement cascades use the displaced directory's deletion
  estimate for affected descendant rows.
- Before upload, all owned transactions, statements, readers, cursors, and
  SQLite connections for the local file are finished or closed.
- A database prepared for upload is a single self-contained `snapshot.db` file.
