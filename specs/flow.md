# Flow

An example run of KitchenSync from start to exit, followed by tricky situations.

## Example: Once Mode

Setup: node A syncs `/home/ace/docs` with two peers — a NAS over SSH and a USB drive.

```
peers.conf:
  sftp://ace@nas.local/volume1/docs
  file:///media/ace/usb-backup/docs
```

Since last run, the user edited `report.txt` and deleted `old/draft.md` locally. Meanwhile, someone added `notes/meeting.txt` on the NAS, and `photos/cat.jpg` was copied to the USB drive.

### 1. Startup

- Open `.kitchensync/kitchensync.sqlite`, init schema, set WAL mode.
- Read `config` table for `"serving-port"` → port 48201. POST `http://127.0.0.1:48201/app-path` → connection refused (previous instance crashed). Proceed.
- Bind `127.0.0.1:0` → OS assigns port 51033. Upsert `"serving-port" = "51033"` in `config` table.
- Connect to NAS (SSH agent auth succeeds on first try). Connect to USB drive (local path, just check it exists).
- Create 10 queues per peer (all empty, no connections yet).
- Print `Listening on port 51033` to stdout. Log `info` to database.

### 2. Walk all devices (concurrently)

Three threads run in parallel — one per device. Each thread reads the device's manifest, walks the filesystem, updates metadata, and builds an in-memory index.

- **Local thread:** read manifest, walk `/home/ace/docs`.
  - `report.txt`: on disk with new size/mtime. In-memory index records current state.
  - `old/draft.md`: in manifest but not on disk → create tombstone `SNAP/old/draft.md` with `del_time` = `reconcile_time`. Remove from manifest.
  - Write updated manifest. Update `reconcile_time`. Clean up one stale XFER directory from the previous crash.
  - **In-memory index:** 49,999 live files + 1 tombstone.
- **NAS thread:** read NAS's manifest over SFTP (~2.5 MB), walk NAS's filesystem (~4 MB directory listing).
  - `notes/meeting.txt`: on disk but not in manifest → add to manifest. In-memory index records it.
  - All other files: on disk, in manifest. Index records current state.
  - Write updated manifest (~2.5 MB). Update `reconcile_time`.
  - **In-memory index:** 50,001 live files + 0 tombstones.
- **USB thread:** read USB's manifest, walk USB's filesystem (local I/O, no SFTP).
  - `photos/cat.jpg`: on disk but not in manifest → add to manifest.
  - **In-memory index:** 50,001 live files + 0 tombstones.

### 3. Compare (in-memory)

Diff local index against each peer's index. Enqueue differing paths.

- **Local vs NAS:**
  - `report.txt`: both live, local `mod_time` newer → enqueue on NAS queue.
  - `old/draft.md`: local deleted, NAS live → enqueue on NAS queue.
  - `notes/meeting.txt`: NAS live, local unknown → enqueue on NAS queue.
  - All other paths match → skip.
- **Local vs USB:**
  - `report.txt`: both live, local `mod_time` newer → enqueue on USB queue.
  - `old/draft.md`: local deleted, USB unknown → skip (no action per decision rules).
  - `photos/cat.jpg`: USB live, local unknown → enqueue on USB queue.
  - All other paths match → skip.

### 4. Queue workers process entries

- **NAS queue workers:**
  - `report.txt`: local newer → push to NAS via XFER staging, displace NAS's copy to NAS's `BACK/`.
  - `old/draft.md`: local `del_time` newer than NAS's `mod_time` → displace NAS's copy to NAS's `BACK/`, create tombstone on NAS.
  - `notes/meeting.txt`: NAS live, local unknown → pull from NAS via local XFER staging.
- **USB queue workers:**
  - `report.txt`: local newer → push to USB via XFER staging.
  - `photos/cat.jpg`: USB live, local unknown → pull from USB via local XFER staging.

Note: `notes/meeting.txt` was pulled locally from the NAS, and `photos/cat.jpg` from the USB drive. Neither is propagated to the other peer in this run — once mode does not propagate changes between peers. A subsequent run will detect these files locally and push them to the peers that don't have them. In watch mode, the watcher would catch them immediately.

### 5. Shutdown

- All queues drained. Log `info` to database. Exit 0.

## Example: Watch Mode

Setup: same as once mode above.

### 1–4. Same as once mode

- Startup, walk, compare, queue processing — identical, except the filesystem watcher starts immediately after startup (before walks begin).

### 5. Steady state

- Watcher detects `todo.txt` created by the user.
- Update local manifest, enqueue `todo.txt` on NAS's shortest queue and USB's shortest queue.
- Queue workers pick it up: local has live file, peers have no entry → push to both via XFER staging.
- Watcher continues until `POST /shutdown` is received.

### 6. Shutdown

- Receive `POST /shutdown` with valid timestamp. Respond `{"shutting_down": true}`.
- Stop the filesystem watcher. Drain remaining queues.
- Log `info` to database. Exit 0.

## Tricky Situations

### Peer unreachable during sync

The NAS is powered off. SSH connection refused → log `error`, skip the NAS entirely (no walk thread, no queues). Only the USB drive gets walked and queued. The NAS will receive the changes on a future run.

### Peer becomes unreachable mid-session

The NAS was reachable during the walk but drops while a queue worker is transferring a file. The worker logs `error` and exits. Other NAS queue workers detect the same failure and exit. Any incomplete XFER directories will be cleaned up on a future run. The NAS catches up later.

### Both sides changed the same file

Node A edited `config.yaml` at 09:00. While A was offline, someone edited `config.yaml` on the NAS at 10:00. During the walk, both in-memory indexes record their respective mtimes. The compare step sees different mtimes and enqueues the path. The queue worker stats both sides: NAS (10:00) is newer than local (09:00). A's version is displaced to local `BACK/`, NAS's version is pulled.

### File deleted on one side, modified on the other

Node A deletes `notes.txt` while offline. The walk creates a tombstone with `del_time` = `reconcile_time` (say, 08:00). The NAS walk finds `notes.txt` with `mod_time` 10:00. The compare enqueues it. The queue worker reads both states: NAS `mod_time` (10:00) is newer than A's `del_time` (08:00) → NAS's version wins. The file is pulled back to A, the tombstone is removed.

If the NAS's version were older (`mod_time` 07:00), A's deletion (08:00) would win → displace the NAS's copy to NAS's `BACK/`.

### Crash during sync

KitchenSync crashes while queue workers are transferring files. On next run, the instance check gets connection refused → proceeds to take ownership. Walks clean up stale XFER directories (older than 2 days) on all devices. The walks rebuild in-memory indexes from current disk state, the compare re-enqueues any unfinished work.

### Nested sync roots

`/tree` is a sync root. `/tree/subtree/` is also a sync root (has its own `.kitchensync/`). The parent walks into `subtree/` and syncs its files normally but skips `subtree/.kitchensync/`. The child syncs independently with its own peers.

### Empty state (first run)

Manifests are empty, no tombstones, no reconcile_time. The walks build in-memory indexes from directory listings. The compare finds every path differs (unknown on one side, live on the other). All files are exchanged — where the same path exists on both sides, the newer `mod_time` wins.

### USB drive plugged in after a month

USB's manifest and tombstones are a month old but internally consistent. Local and NAS have diverged. The USB walk updates nothing (USB's disk matches its manifest). The compare diffs local's index against USB's index and finds many differences — files added, modified, and deleted over the past month. All differences are enqueued and processed. The USB is brought up to date with local in one run.

### Same file exists on two peers

Both the NAS and the USB drive have `readme.txt`, both absent locally. The compare enqueues `readme.txt` on both the NAS queue and the USB queue. Both queue workers try to pull it locally through separate XFER UUIDs. Each writes to its own XFER directory, then moves into place. The second move displaces the first (to `BACK/`). The version with the newer `mod_time` ends up in place. Redundant work, but no corruption.

### Symlinks in the sync root

The walk encounters a symlink, sees it's a symlink, and skips it entirely — it does not appear in the manifest or in-memory index and is not synced. This avoids syncing files outside the sync root, infinite loops, and Windows privilege issues.
