# SnapshotRows:

## Purpose

SnapshotRows owns the row-level rules for the `snapshot` table in one peer's
local temporary snapshot database. It validates row identity values for tracked
paths below the sync root, stores observed file and directory facts, and applies
the state changes that mark entries present, absent, pending copy, copied,
created, displaced, or displaced as part of a directory subtree.

This child does not create, open, validate, or close SQLite files. Callers give
it an already-open local snapshot database whose schema has already been
accepted by the file-handling layer. SnapshotRows mutates only the supplied
database, so every operation affects one peer snapshot and never another peer.

## Responsibilities

SnapshotRows exposes row mutation operations for a valid local `snapshot`
database. Every operation that writes or looks up a row receives the row
identity data for a path below the sync root: `id`, `parent_id`, and
`basename`. `basename` must be the final path component for that row. The sync
root directory itself is not a row, so SnapshotRows rejects an empty root path,
an empty basename, or identity data that cannot represent a child of the sync
root.

SnapshotRows preserves these stored field meanings:

- `mod_time` is the entry modification time last observed on that peer.
- File rows store the file size in bytes as `byte_size`.
- Directory rows store `-1` as `byte_size`.
- A non-NULL `last_seen` records that listing, completed file copy, or
  completed directory creation confirmed the entry present.
- A NULL `last_seen` records a decided destination file copy that has not
  completed.
- A NULL `deleted_time` records an entry that exists.
- A non-NULL `deleted_time` records a tombstone.
- Tombstone rows stay in the `snapshot` table until cleanup owned by another
  child removes them.

SnapshotRows exposes a confirm-present operation. It upserts the row for the
supplied identity with the supplied current `mod_time`, current `byte_size`,
new `last_seen` timestamp, and `deleted_time = NULL`. The operation is used for
peer listings and records the entry as present on that peer.

SnapshotRows exposes a confirm-absent operation. If the row exists and is not a
tombstone, it sets `deleted_time` to that row's existing `last_seen` value and
leaves `last_seen` unchanged. If the row is already a tombstone or no row
exists, the operation succeeds without changing a row.

SnapshotRows exposes an intended-file-copy operation. It upserts the
destination file row with the winning file's `mod_time`, winning file
`byte_size`, and `deleted_time = NULL` before the copy completes. If the row
already exists, its `last_seen` value is preserved. If the row does not exist,
the inserted row has `last_seen = NULL`. If the app exits before the copy
finishes, that row remains in this pending state.

SnapshotRows exposes a complete-file-copy operation. After the queued file copy
succeeds, it updates the destination file row's `last_seen` to the supplied new
timestamp. It does not invent the timestamp and does not copy the file.

SnapshotRows exposes a complete-directory-creation operation. After directory
creation succeeds, it upserts the destination directory row with the supplied
directory `mod_time`, `byte_size = -1`, supplied new `last_seen`, and
`deleted_time = NULL`. Failed directory creation is not recorded by this child;
callers leave any existing row unchanged by not invoking this success
operation.

SnapshotRows exposes a complete-displacement operation. After an entry has
successfully been moved to `BAK/`, it sets that entry row's `deleted_time` to
the row's existing `last_seen` value and leaves `last_seen` unchanged. Failed
displacement is not recorded by this child; callers leave any existing row
unchanged by not invoking this success operation.

SnapshotRows exposes a complete-directory-displacement-cascade operation. It
uses the displaced directory row's deletion estimate, copied from that row's
existing `last_seen`, as the `deleted_time` for each non-tombstone row in the
same peer database that belongs to the displaced subtree. The cascade includes
the displaced directory row, follows `parent_id` links to descendants, does not
change already tombstoned rows, and does not change rows outside the displaced
subtree.

Every mutation is reported as success only after SQLite accepts the write. SQL
errors, rejected row identity data, missing rows for operations that require an
existing row, and transaction failures are reported to the caller. Operations
specified as no-ops, such as confirming absence for an already tombstoned row,
return success without changing data.

## Boundaries

SnapshotRows does not own the SQLite file lifecycle. It does not create
`snapshot.db`, choose journal mode, create schema, validate schema, build
indexes, close connections, or prepare the file for upload.

SnapshotRows does not compute path IDs, split paths, normalize paths, generate
timestamps, compare peers, choose winners, list peer directories, copy files,
create directories, or move entries to `BAK/`. Callers provide identity values,
basename values, observed file facts, and already-generated timestamp strings.

SnapshotRows does not clean old tombstones or obsolete orphan rows. Cleanup is
owned by the cleanup child. SnapshotRows keeps tombstone rows in the table when
it creates them.

SnapshotRows does not coordinate multiple peers. Each call receives one local
snapshot database target and mutates only that database.

## Invariants

- Every stored row represents one tracked path the peer has or has had below
  the sync root.
- The sync root directory itself has no stored row.
- New tombstones copy an existing `last_seen` value into `deleted_time`; they do
  not use a newly generated timestamp.
- Confirm-present and completed directory creation clear `deleted_time`.
- Intended file copy clears `deleted_time` and preserves `last_seen`.
- Completed file copy and completed directory creation write a new `last_seen`
  supplied by the caller.
- Directory displacement cascades use the displaced directory row's deletion
  estimate for all affected non-tombstone descendants in the same database.
