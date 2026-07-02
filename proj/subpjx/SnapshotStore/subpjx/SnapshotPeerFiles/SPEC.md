# SnapshotPeerFiles:

## Purpose

SnapshotPeerFiles owns peer-side snapshot database file exchange for
SnapshotStore. It recovers interrupted peer snapshot replacement state during
normal startup, downloads an existing peer `.kitchensync/snapshot.db` into the
run's local temporary snapshot file, asks the local snapshot database owner to
create that temporary file when the peer has no snapshot history, and uploads a
closed updated temporary snapshot back to the peer through snapshot SWAP
staging.

This child reports whether snapshot startup work left a peer available for the
run. It does not decide canon or subordinate roles, but it must preserve the
startup fact that a reachable peer either had peer snapshot history after SWAP
recovery or needed a new empty local snapshot.

## Responsibilities

SnapshotPeerFiles exposes a startup operation for one connected peer and its
assigned local temporary directory. In a normal run, the operation first
recovers snapshot SWAP state under `.kitchensync/SWAP/snapshot.db/` before it
checks or downloads live snapshot history. The fixed peer paths are:

- live snapshot: `.kitchensync/snapshot.db`
- snapshot SWAP new: `.kitchensync/SWAP/snapshot.db/new`
- snapshot SWAP old: `.kitchensync/SWAP/snapshot.db/old`

Snapshot SWAP recovery applies only these cases:

- If `old` and live `snapshot.db` exist, leave live `snapshot.db` in place and
  delete `old` and any `new`.
- If `old` and `new` exist while live `snapshot.db` is missing, rename `new`
  to live `snapshot.db` and delete `old`.
- If `old` exists while `new` and live `snapshot.db` are both missing, rename
  `old` to live `snapshot.db`.
- If `new` and live `snapshot.db` exist while `old` is missing, leave live
  `snapshot.db` in place and delete `new`.
- If `new` exists while `old` and live `snapshot.db` are both missing, rename
  `new` to live `snapshot.db`.

If snapshot SWAP recovery fails, the startup operation returns an unavailable
result for that peer and leaves any remaining SWAP state for a later normal
startup. The caller uses that result to exclude the peer from the reachable
set.

After successful recovery, the startup operation downloads an existing live
`.kitchensync/snapshot.db` to `{tmp}/{uuid}/snapshot.db`. If the live snapshot
is not found, the operation asks the local snapshot database owner to create a
new empty local snapshot at `{tmp}/{uuid}/snapshot.db` and returns a successful
startup result that records no peer snapshot history. If download fails with
any error other than not found, the operation returns an unavailable result for
that peer so the caller can exclude it from the reachable set.

SnapshotPeerFiles exposes a normal-run upload operation for one connected peer
and one closed local temporary `snapshot.db` file. The operation is used for
contributing and subordinate peers whose updated snapshot data must be written
back in a normal run. It reads bytes from the closed local file, never from a
live SQLite connection, and sends only `snapshot.db` as peer snapshot state.
SQLite sidecar files are not downloaded, uploaded, or treated as peer snapshot
state.

Normal upload uses this sequence:

- Write the replacement database to `.kitchensync/SWAP/snapshot.db/new`.
- Close the peer-side `new` file before replacing the live snapshot.
- If live `.kitchensync/snapshot.db` exists, move it to
  `.kitchensync/SWAP/snapshot.db/old`.
- Move `.kitchensync/SWAP/snapshot.db/new` into live
  `.kitchensync/snapshot.db`.
- Delete `.kitchensync/SWAP/snapshot.db/old` after `new` has become live.

The upload sequence must replace an existing live snapshot on transports whose
`rename(src, dst)` rejects an existing destination. If upload fails before
`old` exists, SnapshotPeerFiles reports the failure without removing the live
snapshot; any leftover `new` is left for the next normal startup recovery. If
upload fails after `old` exists, SnapshotPeerFiles reports the failure and
retains the peer-side snapshot SWAP state for the next normal startup
recovery.

When overlapping normal runs upload snapshots to the same peer, the peer-side
live `.kitchensync/snapshot.db` must reflect the last completed snapshot
upload. A completed upload is one that has moved SWAP `new` into the live
snapshot path.

## Boundaries

SnapshotPeerFiles uses the connected peer transport surface for stat, read,
write, close, rename, and delete operations. It does not connect peers, parse
URLs, choose fallback URLs, authenticate SFTP, or create peer roots.

SnapshotPeerFiles uses the local snapshot database owner to create a new empty
local snapshot database and to ensure a local database is ready to be uploaded.
It does not define the SQLite schema, update snapshot rows, clean snapshot
rows, choose rollback-journal mode, manage SQLite transactions, finalize
statements, or close SQLite connections.

SnapshotPeerFiles does not decide when final upload begins. The caller must
wait until all enqueued file copies have completed and the local snapshot file
has been closed before invoking upload.

SnapshotPeerFiles does not upload snapshots during dry-run. Dry-run callers may
use local temporary snapshots prepared by other SnapshotStore behavior, but
peer-side snapshot SWAP recovery and peer-side snapshot upload are normal-run
mutations only.

SnapshotPeerFiles does not decide global peer reachability or peer roles. It
returns structured startup outcomes that distinguish recovered-and-downloaded,
recovered-with-new-empty-local-snapshot, and unavailable due to recovery or
download failure. The caller removes unavailable peers from the run and applies
canon, subordinate, and contributing rules.

SnapshotPeerFiles does not recover user-data SWAP directories, create BAK or
TMP paths, clean BAK or TMP directories, copy user files, set user-file
modification times, update snapshot rows after user-file work, or format
stdout diagnostics.

## Invariants

- Peer snapshot state is the single file `.kitchensync/snapshot.db`.
- Snapshot SWAP state for snapshot replacement uses only
  `.kitchensync/SWAP/snapshot.db/new` and
  `.kitchensync/SWAP/snapshot.db/old`.
- Snapshot SWAP recovery runs before deciding whether the peer has snapshot
  history during normal startup.
- Startup recovery or download failure makes only that peer unavailable for
  snapshot startup.
- A missing live peer snapshot after successful recovery creates a new empty
  local `{tmp}/{uuid}/snapshot.db` and records that the peer had no snapshot
  history at startup.
- Peer SQLite sidecar files are not snapshot state.
- Upload reads the closed local temporary `snapshot.db` file.
- Upload closes peer-side SWAP `new` before moving it into the live snapshot
  path.
- Interrupted upload state is recovered by the next normal startup rather than
  purged by age.
