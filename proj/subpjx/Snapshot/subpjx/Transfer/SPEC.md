# Transfer:

## Purpose

Transfer owns the snapshot database file as it moves between a peer and the local
temporary working directory. It is the file-level half of Snapshot: it never
reads or writes rows, never knows the schema, and never computes an identity or a
timestamp. Its single concern is the bytes of `snapshot.db` and the SWAP state
machine that gets those bytes safely onto a peer.

It exists so that the SQL row logic in Store has a stable local file to open and
so that the rest of the run never touches a peer's live `snapshot.db` in place.
Every peer-touching step that involves the snapshot file -- download, startup
recovery, and writeback upload -- happens here and nowhere else. The Transport
component does the actual reads, writes, renames, and deletes against the peer;
Transfer decides the sequence of those operations.

## Responsibilities

The operations Transfer exposes across its boundary fall into three groups.

Download and local placement:

- Treat each peer's snapshot as living at `{peer-root}/.kitchensync/snapshot.db`,
  a rollback-journal SQLite file, and never upload its SQLite sidecar files: only
  `snapshot.db` itself is part of a peer's state (016.1, 016.2, 016.3).
- In a normal run, download a peer's live `.kitchensync/snapshot.db` through
  Transport to a fresh local temporary path `{tmp}/{uuid}/snapshot.db`, where each
  run gets its own `uuid` directory, and leave the peer copy untouched until
  writeback (016.4, 016.5).
- When Transport reports the peer has no snapshot ('not found'), create a new
  empty snapshot database at the local temporary path so the caller has a file to
  open (016.6). Transfer creates only the empty file; it does not create any table
  or schema -- that belongs to Store.
- Report to the caller whether the peer had existing snapshot history, but only
  after SWAP recovery has been applied, never before (016.13).

Startup SWAP recovery (applied before deciding whether a peer has history, and
before download):

- `old` exists and `snapshot.db` exists: delete `new` if present, then delete
  `old` (016.14).
- `old` exists, `new` exists, `snapshot.db` missing: rename `new` to
  `snapshot.db`, then delete `old` (016.15).
- `old` exists, `new` missing, `snapshot.db` missing: rename `old` to
  `snapshot.db` (016.16).
- `old` missing, `new` exists, `snapshot.db` exists: delete `new` (016.17).
- `old` missing, `new` exists, `snapshot.db` missing: rename `new` to
  `snapshot.db` (016.18).

SWAP-staged writeback upload:

- The file handed to Transfer for upload is already a self-contained
  rollback-journal SQLite database with all of the run's changes committed and
  every connection, statement, and cursor closed; Transfer uploads it as-is and
  relies on the caller to have made it self-contained (016.7).
- Write and close the new database at `.kitchensync/SWAP/snapshot.db/new` (016.8).
- Rename the live `.kitchensync/snapshot.db` to `.kitchensync/SWAP/snapshot.db/old`
  when the live file exists (016.9).
- Rename `new` to `.kitchensync/snapshot.db` (016.10).
- Delete `old` after the new snapshot is in place (016.11).
- Never rely on rename-over-existing: every rename targets a name that does not
  already exist, so replacement succeeds on transports whose `rename(src, dst)`
  rejects an existing destination (016.12).

Dry-run handling:

- Under `--dry-run`, skip the startup peer-side SWAP recovery entirely (024.2).
- Under `--dry-run`, still download each reachable peer's live
  `.kitchensync/snapshot.db` as-is to the local temp path (024.3).
- Under `--dry-run`, do not upload the updated local temp snapshot back to any
  peer: skip the SWAP-staged writeback entirely so no peer snapshot state changes
  (024.18).

## Boundaries

Operations across the boundary:

- Download a peer's snapshot to a local temp path, returning that path and whether
  the peer had history (after recovery).
- Run startup SWAP recovery for a peer.
- Upload a local database file back to a peer through the SWAP-staged writeback.
- Each of the above honors the dry-run flag the caller passes in.

Error obligations:

- Transfer surfaces the transport error categories raised while downloading,
  recovering, or uploading `snapshot.db` to its caller; it does not decide whether
  such a failure aborts the run.
- When upload fails before `old` exists, the live `snapshot.db` is kept and any
  SWAP `new` is left in place for the next run's startup recovery (016.20).
- When upload fails after `old` exists, the SWAP state is left exactly as it is and
  recovered on the next normal run (016.21). Transfer does not roll a peer back
  beyond leaving recoverable SWAP state.

Invariants:

- A peer's `.kitchensync/snapshot.db` is never modified in place during a run; all
  changes reach the peer only through the SWAP-staged writeback (016.5, 016.8
  through 016.12).
- Recovery is always applied before history is determined and before download
  (016.13).
- When two runs overlap, the peer's final `snapshot.db` is the one written by the
  run that uploads last; Transfer does not lock or coordinate between runs (016.19).
- No rename ever targets an existing name (016.12).

What Transfer does not do:

- It does not open the database, read or write rows, create the schema, or commit
  transactions; it moves and renames the file and trusts the caller to hand it a
  self-contained, committed database for upload.
- It does not compute path identities or timestamps.
- It does not own the dry-run decision; the caller hands it the flag and Transfer
  honors it for its own peer-touching steps (024.2, 024.3, 024.18).

Transfer reaches the peer only through the Transport instance its parent supplies;
it issues no network or filesystem calls against a peer directly.
