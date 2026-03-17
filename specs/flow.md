# Flow

An example run of KitchenSync from start to exit, followed by tricky situations.

## Example: Once Mode

Setup: node A syncs `/home/bilbo/docs` with two peers -- a NAS and a USB drive.

```
peers.conf:
nas
  sftp://bilbo@192.168.1.50/volume1/docs
  sftp://bilbo@nas.tail12345.ts.net/volume1/docs

usb-backup
  file:///media/bilbo/usb-backup/docs
```

Since last run, the user edited `report.txt` and deleted `old/draft.md` locally. Meanwhile, someone added `notes/meeting.txt` on the NAS, and `photos/cat.jpg` was copied to the USB drive.

### 1. Startup

- Open `.kitchensync/kitchensync.db`, init schema, set WAL mode.
- Instance check: read `config` table for `"serving-port"` -> POST `/app-path` -> connection refused. Proceed.
- Bind `127.0.0.1:0` -> OS assigns port 51033. Upsert `"serving-port" = "51033"`.
- Log `info` startup message to database.
- Peer databases in `.kitchensync/PEER/` are preserved (queues persist).

### 2. Local Walker

Runs immediately (no watcher in once mode).

- Phase 1: Walk `/home/bilbo/docs` filesystem
  - `report.txt`: on disk with new size/mtime. Database shows old values. Update database row, enqueue to NAS and USB SQLite queues.
  - Other files: match database. Skip.
- Phase 2: Walk database for deletions
  - `old/draft.md`: in database with no `del_time`, not on disk. Set `del_time`, enqueue to NAS and USB.
- Local walker completes. Paths enqueued to peer SQLite databases.

### 3. Connection Managers (concurrently)

Two connection manager threads start, one for NAS and one for USB.

**NAS connection manager:**
- Try first URL `192.168.1.50` -> succeeds.
- Walk NAS filesystem over SFTP, update snapshot table:
  - `notes/meeting.txt`: on NAS, not in local database. Enqueue to NAS (and USB, since once mode).
  - Other files: record in snapshot, skip if matches local.
- Spawn `workers-per-peer` (default: 10) workers to drain NAS queue.

**USB connection manager:**
- Try `file://` path -> exists.
- Walk USB filesystem, update snapshot table:
  - `photos/cat.jpg`: on USB, not in local database. Enqueue to USB (and NAS, since once mode).
  - Other files: record in snapshot, skip if matches local.

### 4. Workers Drain Queues

**NAS workers (`workers-per-peer` threads):**
- `report.txt`: local database shows newer mtime than NAS snapshot. Push to NAS via XFER staging, displace NAS copy to NAS's `BACK/`.
- `old/draft.md`: local `del_time` set, NAS still has file. Push deletion: displace NAS copy to NAS's `BACK/`.
- `notes/meeting.txt`: NAS has file, local unknown. Pull from NAS via local XFER staging, update local database.

**USB workers (`workers-per-peer` threads):**
- `report.txt`: local newer than USB. Push to USB via XFER staging.
- `photos/cat.jpg`: USB has file, local unknown. Pull from USB, update local database.
- `notes/meeting.txt`: enqueued from NAS walker. Local now has it (pulled from NAS). Push to USB.
- `old/draft.md`: enqueued from local walker. Local `del_time` set, USB unknown. No action needed.

### 5. Shutdown

- All connection managers complete their cycle (connect, walk, drain, disconnect).
- Log `info` to database. Exit 0.
- Peer databases (including empty queues) remain in `.kitchensync/PEER/`.

## Example: Watch Mode

Setup: same as once mode above.

### 1. Startup

Same as once mode, plus start the filesystem watcher immediately.

### 2. Local Walker + Connection Managers

- Watcher starts, monitoring for filesystem changes.
- Local walker runs, enqueues differences to peer SQLite queues.
- Connection managers start (one per peer), begin connect/walk/drain cycles.

Watcher events for paths being walked may produce redundant enqueues -- harmless (deduped by path).

### 3. Steady State

- User creates `todo.txt`.
- Watcher detects it, updates local database, enqueues to NAS and USB SQLite queues.
- Connection managers see non-empty queues, connect, drain them.
- When queues are empty, connection managers wait for either: new queue entries, or time to re-walk (per `rewalk-after-minutes`).
- Periodic re-walks catch any external changes on peers that don't run KitchenSync.

### 4. Shutdown

- Receive `POST /shutdown` with valid timestamp. Respond `{"shutting_down": true}`.
- Stop the watcher. Signal connection managers to finish current cycle and exit.
- Wait for all connection managers to complete.
- Log `info` to database. Exit 0.

## Tricky Situations

### Peer unreachable at startup

The NAS is powered off. NAS connection manager tries all URLs, all fail -> log warning, sleep `retry-interval` seconds (default: 60), retry. Meanwhile, changes accumulate in NAS's SQLite queue. When NAS powers on, next connection attempt succeeds, peer walk runs, queue drains. Fast catch-up.

### Peer becomes unreachable mid-transfer

NAS was reachable during walk but drops during queue processing. Workers log `error` and exit. Connection manager detects the dropped connection, retries every `retry-interval` seconds (queue is still non-empty). When NAS is reachable again, reconnects and drains remaining queue. Incomplete XFER directories cleaned up on next walk.

### Both sides changed the same file

Node A edited `config.yaml` at 09:00. Someone edited it on NAS at 10:00. Walker detects difference, enqueues path. Queue worker compares local database (09:00) to `nas.db` (10:00). NAS is newer. Local copy displaced to `BACK/`, NAS version pulled.

### File deleted on one side, modified on the other

Node A deletes `notes.txt` while offline. Local walker sets `del_time` to current time (say, 08:00). NAS walker finds `notes.txt` with mtime 10:00. Queue worker compares: NAS mtime (10:00) > local `del_time` (08:00). NAS wins. File pulled back, `del_time` cleared.

If NAS version were older (mtime 07:00), deletion wins -> NAS copy displaced to NAS's `BACK/`.

### Crash during sync

KitchenSync crashes while transferring files. On next run:
- Instance check gets connection refused -> take ownership
- `PEER/` directory preserved -- queues survive the crash
- Local walker re-enqueues any local changes (may duplicate, deduped by path)
- Connection managers reconnect and continue processing
- XFER directories older than `xfer-cleanup-days` (default: 2) cleaned up
- BACK directories older than `back-retention-days` (default: 90) cleaned up
- Tombstones older than `tombstone-retention-days` (default: 180) cleaned up

### Nested sync roots

`/tree` is a sync root. `/tree/subtree/` is also a sync root. The parent walks into `subtree/` and syncs its files normally but skips `subtree/.kitchensync/`. Each operates independently with its own database.

### Empty state (first run)

Local database is empty. Local walker builds it from filesystem. Connection managers connect to peers, walk them, enqueue differences. Everything gets reconciled. Where same path exists on multiple devices, newer mtime wins.

### USB drive plugged in after a month

USB's files are a month old. While USB was offline, changes accumulated in USB's SQLite queue. When USB is plugged in:
- USB connection manager detects `file://` path now exists
- Connects, walks USB filesystem, updates snapshot
- Drains queue (recent changes sync first, queue was capped at 10,000)
- Peer walk catches any older changes that overflowed the queue
- USB brought up to date

### Same file exists on two peers

Both NAS and USB have `readme.txt`, local doesn't. Both peer walkers enqueue it. Both queue workers try to pull through separate XFER UUIDs. Second move displaces the first to `BACK/`. Newer mtime wins. Redundant work, no corruption.

### Symlinks in the sync root

Walker encounters symlink -> skips it. Symlinks do not appear in databases and are not synced.

### Fast fan-out on startup

Local database already has 50,000 files from previous run. User edited 5 files since then. Local walker:
1. Walks filesystem, comparing to database
2. Detects 5 changed files within seconds
3. Enqueues all 5 to all peers immediately

No need to wait for peer walks. Outgoing updates begin within seconds of start.

### Directory renamed (deep subtree)

User renames `projects/old-name/` to `projects/new-name/`. The directory contains 500 files in nested subdirectories.

KitchenSync does not track renames -- it sees paths. This looks like 500 deletions and 500 creations:

**Local walker Phase 1 (walk filesystem):**
- Encounters all 500 files under `projects/new-name/`
- None exist in database (new paths) -> insert rows, enqueue to all peers

**Local walker Phase 2 (walk database):**
- Finds all 500 rows under `projects/old-name/`
- None exist on disk -> set `del_time` on each, enqueue to all peers

Both phases are required to see the full picture. Phase 1 alone would miss the deletions; Phase 2 alone would miss the creations.

**Result:**
- All 500 "new" files are transferred to peers (full content, not a rename optimization)
- All 500 "old" files are deleted from peers (moved to peers' `BACK/`)
- Bandwidth used: 500 files transferred, even though content is identical

This is a known limitation of path-based sync. Content-addressed systems (like git or rsync with `--fuzzy`) can detect renames, but add significant complexity. KitchenSync prioritizes simplicity -- renames are rare compared to edits, and the result is correct even if not optimal.
