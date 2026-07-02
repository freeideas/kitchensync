# SnapshotFile:

## Purpose

SnapshotFile owns the local temporary SQLite `snapshot.db` file boundary for
one peer during a KitchenSync run. It creates a new empty local snapshot
database, opens an existing downloaded local snapshot database, validates the
required SQLite schema, keeps all reads and writes pointed at the caller's
local temporary file, and closes the database so transport upload can read the
file directly.

SnapshotFile is the part of SnapshotDatabase that makes the snapshot file safe
to pass between SQLite and transport code. Every database it creates or updates
uses SQLite rollback-journal mode, not WAL mode, and a database prepared for
upload is a self-contained `snapshot.db` file with no SQLite sidecar files
needed for later use.

## Responsibilities

SnapshotFile exposes an operation to create a new empty snapshot database at a
caller-supplied local temporary `snapshot.db` path. The operation opens that
file with SQLite, sets rollback-journal behavior, creates exactly the required
schema, and validates that the created file matches the required shape before
returning an open database handle to the caller.

SnapshotFile exposes an operation to open an existing caller-supplied local
temporary `snapshot.db` path. The operation sets rollback-journal behavior for
the opened database, validates the schema, and returns an open database handle
only when the file already satisfies the required shape.

SnapshotFile exposes a schema validation operation for an open local snapshot
database. Validation succeeds only when the database contains exactly one table
named `snapshot`, contains no views, and has the exact required table columns:

- `id` with SQLite type `TEXT` and primary-key status.
- `parent_id` with SQLite type `TEXT`.
- `basename` with SQLite type `TEXT` and `NOT NULL`.
- `mod_time` with SQLite type `TEXT` and `NOT NULL`.
- `byte_size` with SQLite type `INTEGER` and `NOT NULL`.
- `last_seen` with SQLite type `TEXT` and nullable status.
- `deleted_time` with SQLite type `TEXT` and nullable status.

The `snapshot` table must have indexes that cover `parent_id`, `last_seen`, and
`deleted_time`. SnapshotFile must not create or accept extra application
tables, alternate table names, views, missing columns, extra columns, wrong
column types, wrong nullability, a missing primary key on `id`, or missing
required indexes.

SnapshotFile exposes the open SQLite handle used by SnapshotDatabase's row and
cleanup children. During a run, all snapshot reads and writes through that
handle use the peer's caller-supplied local temporary `snapshot.db` copy.
SnapshotFile does not redirect those operations to the peer-side
`.kitchensync/snapshot.db` path.

SnapshotFile exposes a prepare-for-upload operation for an open local temporary
snapshot database. Before it returns success, the operation commits or rolls
back every transaction that SnapshotFile owns for that local file, finalizes
every statement, cursor, and reader that SnapshotFile owns for that local file,
and closes every SQLite connection that SnapshotFile owns for that local file.
After success, transport upload reads the closed local file from the filesystem,
not a live SQLite connection, and the uploaded peer-side
`.kitchensync/snapshot.db` is usable as a self-contained SQLite database.

SnapshotFile reports errors from SQLite open, rollback-journal setup, schema
creation, schema validation, transaction finish, statement finalization, reader
or cursor finish, and connection close. It must not report success when the
local database file is not in the required schema, when it cannot force
rollback-journal behavior, or when any owned SQLite resource for the file
remains open before upload.

## Boundaries

SnapshotFile does not download, upload, recover, rename, delete, or stage
peer-side snapshot files. It works only on the caller's local temporary
`snapshot.db` path and leaves peer transport behavior to other children.

SnapshotFile does not mutate snapshot rows for listings, copies, directory
creation, absence, displacement, or cleanup. It provides the validated SQLite
file and open handle that those row-level operations use.

SnapshotFile does not compute path IDs, normalize paths, generate timestamps,
interpret `byte_size`, decide tombstone state, decide cleanup cutoffs, or
format progress output. It only enforces the SQLite file lifecycle, schema, and
close-before-upload rules assigned to this child.

SnapshotFile does not own SQLite resources created wholly by a caller outside
its boundary. Callers that create additional statements, cursors, readers,
transactions, or connections must finish them before asking SnapshotFile to
prepare the file for upload, or SnapshotFile must report that the file cannot be
prepared.

## Invariants

- Every database created or updated through SnapshotFile uses SQLite
  rollback-journal mode.
- SnapshotFile operates on the peer's local temporary `snapshot.db` copy during
  the run.
- The database has exactly one `snapshot` table, no views, the seven required
  columns, and indexes on `parent_id`, `last_seen`, and `deleted_time`.
- SnapshotFile rejects schema drift instead of adapting or extending it.
- A successful prepare-for-upload leaves no owned open transaction, statement,
  cursor, reader, or SQLite connection for the local file.
- A successful prepare-for-upload leaves a closed, self-contained
  `snapshot.db` file that does not require SQLite sidecar files.
