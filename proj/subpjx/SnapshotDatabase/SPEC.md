# SnapshotDatabase:

## Purpose

SnapshotDatabase owns each peer's SQLite snapshot database as a local working
file and as a peer metadata artifact. It creates the exact snapshot schema,
loads a reachable peer's live snapshot into a local temporary
`snapshot.db`, applies row updates during sync, cleans obsolete rows
opportunistically, and publishes the updated closed database back to the peer
through the snapshot SWAP path.

This child is used by startup, traversal, copy completion handling, and final
snapshot upload. Callers pass connected peer handles through
PeerTransportSurface, valid relative slash paths and snapshot identifiers from
FormatRules, and timestamp strings generated or parsed by FormatRules.
SnapshotDatabase stores and updates those values without redefining their
format.

## Responsibilities

SnapshotDatabase exposes operations for these behaviors:

- Prepare one reachable peer snapshot for a run. In a normal run this first
  recovers incomplete `.kitchensync/SWAP/snapshot.db/` state on the peer, then
  downloads `.kitchensync/snapshot.db` to a local temporary
  `{tmp}/{uuid}/snapshot.db`. If the live peer snapshot is not found, it
  creates a new empty local temporary SQLite snapshot database and reports that
  the peer had no snapshot history at startup.
- Create a snapshot database in rollback-journal mode. A created database has
  exactly one application table named `snapshot`, no view or alternate
  snapshot table, and the columns `id TEXT PRIMARY KEY`, `parent_id TEXT`,
  `basename TEXT NOT NULL`, `mod_time TEXT NOT NULL`,
  `byte_size INTEGER NOT NULL`, nullable `last_seen TEXT`, and nullable
  `deleted_time TEXT`. It creates non-primary indexes on `snapshot(parent_id)`,
  `snapshot(last_seen)`, and `snapshot(deleted_time)`.
- Open each peer's local temporary `snapshot.db` for all snapshot reads and
  writes during the run. Peer-side SQLite sidecar files are never treated as
  snapshot state and are never uploaded or synced.
- Read snapshot rows needed by reconciliation. A row lookup returns the stored
  path identity fields, modification time, byte size, last-seen timestamp, and
  deleted-time timestamp for that peer only.
- Record a listed file as present by writing the listed modification time,
  listed byte size, a fresh current `last_seen` timestamp, and
  `deleted_time = NULL`.
- Record a listed directory as present by writing the listed modification time,
  `byte_size = -1`, a fresh current `last_seen` timestamp, and
  `deleted_time = NULL`.
- Record a peer that already has the winning file state as confirmed present
  without requiring a copy.
- Record an intended destination file copy before the copy completes by
  writing the winning modification time and byte size and clearing
  `deleted_time`. If the row is new, `last_seen` remains `NULL`. If the row
  already exists, its existing `last_seen` value is preserved.
- Record a successful file copy by setting that destination row's `last_seen`
  to a fresh current timestamp. If the copy does not complete successfully,
  SnapshotDatabase leaves that row's `last_seen` unchanged.
- Record a successfully created destination directory by writing
  `byte_size = -1`, `deleted_time = NULL`, and a fresh current `last_seen`
  timestamp. If the directory creation fails, callers must not ask
  SnapshotDatabase to change the existing row.
- Record confirmed absence only when the peer's row is untombstoned. The
  operation copies that row's existing `last_seen` into `deleted_time` and
  leaves `last_seen` unchanged. If the row is already tombstoned, the row is
  left unchanged.
- Record a successful displacement to BAK by setting the displaced row's
  `deleted_time` to its existing `last_seen`. If the displaced entry is a
  directory, the same operation tombstones untombstoned descendant rows in
  that same peer database, using the displaced entry's copied deletion
  estimate for every descendant it tombstones.
- Keep displacement cascades peer-local. Updating a displaced subtree on one
  peer never changes another peer's snapshot database.
- Leave already tombstoned descendant rows unchanged during a displacement
  cascade, and leave rows outside the displaced subtree unchanged.
- Skip row changes for an unreachable peer and for a directory subtree whose
  live subtree evidence could not be fully listed after the configured listing
  tries.
- Run opportunistic snapshot cleanup without delaying the first directory scan
  or the first eligible file copy. Cleanup removes tombstone rows whose
  `deleted_time` is older than `--keep-del-days` days. It removes an
  untombstoned stale row only when the caller has established that the row is
  obsolete and its `last_seen` is older than `--keep-del-days` days or is
  `NULL`.
- Before uploading a local temporary snapshot database, finish all SQLite work
  against that file: transactions are committed or rolled back, statements and
  readers are finalized, and every connection to that local file is closed.
  The upload reads only the closed `snapshot.db` file, with no required SQLite
  sidecar.
- Upload an updated snapshot only after all enqueued file copies have
  completed. In a normal run, upload writes and closes
  `.kitchensync/SWAP/snapshot.db/new`, moves an existing live
  `.kitchensync/snapshot.db` to `.kitchensync/SWAP/snapshot.db/old`, moves
  `new` into `.kitchensync/snapshot.db`, then deletes `old`.

Snapshot SWAP recovery follows these cases during normal startup:

- If `old` and live `snapshot.db` exist, delete `new` if present, then delete
  `old`.
- If `old` and `new` exist and live `snapshot.db` is missing, rename `new` to
  live `snapshot.db`, then delete `old`.
- If `old` exists and both `new` and live `snapshot.db` are missing, rename
  `old` to live `snapshot.db`.
- If `new` and live `snapshot.db` exist and `old` is missing, delete `new`.
- If `new` exists and both `old` and live `snapshot.db` are missing, rename
  `new` to live `snapshot.db`.

Snapshot replacement must not require a transport rename onto an existing
destination path. If upload fails before SWAP `old` exists, SnapshotDatabase
reports an error-level diagnostic obligation for that upload failure. If upload
fails after SWAP `old` exists, it also reports an error-level diagnostic
obligation and leaves the incomplete snapshot SWAP state on the peer for the
next normal startup recovery.

If snapshot SWAP recovery or snapshot download fails for any reason other than
a missing live `.kitchensync/snapshot.db`, SnapshotDatabase reports an
error-level diagnostic obligation and returns a result that excludes that peer
from the reachable set.

The row operations preserve the intended-copy rule used on later runs: an
absent destination file with an untombstoned intended-copy row whose
`last_seen` is `NULL` or not more than 5 seconds newer than the source
modification time is still treated as a copy target, not as proof that the
source file should be tombstoned. SnapshotDatabase must expose the stored row
values needed for the traversal decision to make that comparison.

## Boundaries

SnapshotDatabase does not parse command-line options, decide verbosity, print
help, print completion output, or format general diagnostics. It returns
operation results and diagnostic facts so the product output owner can emit
stdout-only error-level diagnostics.

SnapshotDatabase does not decide peer reachability, fallback URL order, canon
status, subordinate status, first-sync validity, or whether a run may continue
after a peer is excluded. PeerConnections owns those startup decisions using
SnapshotDatabase's prepare result.

SnapshotDatabase does not implement local filesystem or SFTP operations
directly. All peer reads, writes, renames, deletes, directory creation, and
stats go through PeerTransportSurface. SnapshotDatabase owns only the
snapshot-specific paths and ordering for those peer mutations.

SnapshotDatabase does not own user-file SWAP, BAK, TMP staging, file transfer
scheduling, copy retries, copy progress output, or displacement execution.
CopyStaging and traversal callers perform those user-file operations and call
SnapshotDatabase only after a listed state, intended copy, completed copy,
created directory, confirmed absence, or successful displacement should affect
a peer's snapshot rows.

SnapshotDatabase does not generate snapshot IDs, parent IDs, normalized
relative paths, timestamps, or timestamp comparisons. FormatRules owns path
hashing, basename and parent identity derivation, timestamp formatting,
current timestamp generation, parsing, age cutoffs, and 5-second tolerance
comparisons. SnapshotDatabase persists those values and uses caller-provided
cutoffs and identifiers to select rows.

SnapshotDatabase does not modify snapshot rows for excluded paths, unreachable
peers, failed directory subtrees, failed file copies, failed directory
creations, or failed displacements. Callers signal only confirmed events that
are allowed to update rows.

Its invariants are:

- Every peer prepared for sync has exactly one local temporary `snapshot.db`
  used for that peer's snapshot reads and writes.
- Created and updated snapshot databases remain SQLite rollback-journal
  databases whose upload artifact is the single closed `snapshot.db` file.
- The peer snapshot state path is exactly `.kitchensync/snapshot.db`; SQLite
  sidecar files are not peer snapshot state.
- Snapshot rows are peer-local. A row update for one peer never writes another
  peer's database.
- A new intended-copy row has `last_seen = NULL` until that copy succeeds.
- A reused intended-copy row preserves `last_seen` until that copy succeeds.
- Tombstone writes copy an existing row timestamp; they do not call for a new
  current timestamp.
- Snapshot uploads are attempted only after the copy queue has completed.
- Snapshot replacement is published only by moving SWAP `new` into the live
  path after any existing live snapshot has been moved to SWAP `old`.
