# SnapshotCleanup:

## Purpose

SnapshotCleanup owns opportunistic cleanup for old rows in one peer's local
temporary snapshot database. It removes tombstones whose deletion estimate is
outside the caller's deletion-retention window, and it removes obsolete
non-tombstone rows that were stranded because a directory displacement cascade
can no longer reach them.

This child does not create, open, validate, or close SQLite files. Callers give
it an already-open local snapshot database whose schema has already been
accepted by the file-handling layer. SnapshotCleanup mutates only the supplied
database, so cleanup for one peer never changes another peer's snapshot.

## Responsibilities

SnapshotCleanup exposes a cleanup operation for a valid local `snapshot`
database. The caller supplies the cutoff timestamp derived from
`--keep-del-days`. The operation treats stored timestamp strings as sortable
snapshot timestamps and removes only rows older than that cutoff under the
rules below.

The cleanup operation removes tombstone rows where `deleted_time IS NOT NULL`
and `deleted_time` is older than the cutoff. Rows with `deleted_time` equal to
or newer than the cutoff remain in the table. Rows with `deleted_time IS NULL`
are not removed by the tombstone cleanup rule.

The cleanup operation also removes obsolete orphan rows where
`deleted_time IS NULL`, `last_seen` is older than the same cutoff, and the row
cannot be reached by a directory displacement cascade because the parent chain
needed by that cascade has already been broken. This covers descendants left
behind when an intermediate snapshot row was purged before a later cascade
could walk through it. A row directly below the sync root is not an orphan only
because the sync root itself has no row.

SnapshotCleanup is maintenance work. Callers may run it while traversal has
already begun, after copy work has started, or in another bounded maintenance
window. Sync decisions must not depend on SnapshotCleanup finishing during the
current run, and SnapshotCleanup must not require callers to block the decision
path before they can continue.

SnapshotCleanup reports SQLite delete errors and transaction failures to the
caller. It must not report success for a cleanup pass whose required database
writes were rejected. If no rows match the cleanup rules, the operation
succeeds without changing the database.

## Boundaries

SnapshotCleanup does not own the SQLite file lifecycle. It does not create
`snapshot.db`, choose journal mode, create schema, validate schema, build
indexes, close connections, or prepare the file for upload.

SnapshotCleanup does not mutate rows for listings, intended file copies,
completed file copies, directory creation, confirmed absence, entry
displacement, or directory displacement cascades. It only removes rows that are
eligible under the old-tombstone and obsolete-orphan cleanup rules.

SnapshotCleanup does not compute path IDs, normalize paths, generate
timestamps, parse command-line options, decide the `--keep-del-days` value, or
format progress output. Callers provide the cutoff timestamp derived from the
configured retention window.

SnapshotCleanup does not list peer directories, inspect the live filesystem,
copy files, create directories, move entries to `BAK/`, or decide sync
outcomes. Cleanup is based only on the rows already present in the supplied
local snapshot database.

## Invariants

- Cleanup mutates only the caller-supplied local snapshot database.
- Tombstone cleanup removes only rows with `deleted_time` older than the
  caller-supplied cutoff.
- Obsolete-orphan cleanup removes only non-tombstone rows with `last_seen`
  older than the same cutoff and a broken cascade reachability chain.
- Rows that are still inside the retention window remain available as snapshot
  evidence for later sync decisions.
- Sync correctness must not depend on cleanup completing in the current run.
