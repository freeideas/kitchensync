# 016_snapshot-storage: Snapshot download, upload, and recovery

## Behavior
This concern derives from `specs/database.md` (the introductory storage
paragraphs and "Snapshot SWAP recovery"), `specs/sync.md` section "Rename
Compatibility" (the snapshot replacement paragraph), and the snapshot
download/recovery steps of `specs/sync.md` section "Startup" (step 5).

It covers how each peer's snapshot lives and moves: stored at
`{peer-root}/.kitchensync/snapshot.db` as a rollback-journal SQLite file, with
SQLite sidecar files never synced; downloaded to a local temp directory where all
reads and writes happen; created as a new empty database locally when a peer has
none. It covers the requirement that all database work is committed/rolled back
and all connections, statements, and cursors are closed before the
self-contained file is uploaded. It covers writeback through the snapshot SWAP
path (`.kitchensync/SWAP/snapshot.db/new` and `old`): write and close `new`,
rename live `snapshot.db` to `old`, rename `new` into place, delete `old`, on
transports that reject rename-over-existing. It covers the five snapshot SWAP
recovery states applied before deciding whether a peer has history, the
last-upload-wins behavior of overlapping runs, and the upload-failure handling
(before/after `old` exists, SWAP state left for next-run recovery).

The same SWAP discipline applied to user files is `019_swap-replacement`. The
overall run ordering of download-then-walk-then-upload is `006_run-lifecycle`.
Dry-run suppression of upload and peer-side recovery is `024_dry-run`.

## $REQ_IDs

- `016.1` -- Each peer's snapshot is stored at `{peer-root}/.kitchensync/snapshot.db`.
- `016.2` -- The peer-stored `snapshot.db` is a SQLite database in rollback-journal mode.
- `016.3` -- SQLite sidecar files are never uploaded to peers; only `snapshot.db` is part of peer state.
- `016.4` -- In a normal run, each peer's `snapshot.db` is downloaded to a local temporary path `{tmp}/{uuid}/snapshot.db`.
- `016.5` -- Snapshot changes during a run are applied to the downloaded local copy, and the peer's `.kitchensync/snapshot.db` is not modified in place before writeback.
- `016.6` -- When a peer has no existing `snapshot.db` (transport returns 'not found'), a new empty snapshot database is created locally.
- `016.7` -- The `snapshot.db` uploaded to a peer is a self-contained rollback-journal SQLite file that opens standalone with all of the run's snapshot changes committed.
- `016.8` -- Snapshot writeback writes the new database to `.kitchensync/SWAP/snapshot.db/new`.
- `016.9` -- Snapshot writeback renames the live `.kitchensync/snapshot.db` to `.kitchensync/SWAP/snapshot.db/old` when the live file exists.
- `016.10` -- Snapshot writeback renames `new` to `.kitchensync/snapshot.db`.
- `016.11` -- Snapshot writeback deletes `old` after the new snapshot is in place.
- `016.12` -- Snapshot replacement succeeds on transports whose `rename(src, dst)` rejects an existing destination, without relying on rename-over-existing.
- `016.13` -- Snapshot SWAP recovery is applied before deciding whether a peer has snapshot history.
- `016.14` -- Recovery when `old` exists and `snapshot.db` exists: `new` is deleted if present, then `old` is deleted.
- `016.15` -- Recovery when `old` exists, `new` exists, and `snapshot.db` is missing: `new` is renamed to `snapshot.db`, then `old` is deleted.
- `016.16` -- Recovery when `old` exists, `new` is missing, and `snapshot.db` is missing: `old` is renamed to `snapshot.db`.
- `016.17` -- Recovery when `old` is missing, `new` exists, and `snapshot.db` exists: `new` is deleted.
- `016.18` -- Recovery when `old` is missing, `new` exists, and `snapshot.db` is missing: `new` is renamed to `snapshot.db`.
- `016.19` -- When two runs overlap, the peer's final `snapshot.db` is the one written by the run that uploads last.
- `016.20` -- When snapshot upload fails before `old` exists, the live `snapshot.db` is kept and any SWAP `new` is left for startup recovery.
- `016.21` -- When snapshot upload fails after `old` exists, the SWAP state is left in place and recovered on the next normal run.
