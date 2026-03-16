# Sync

How KitchenSync synchronizes files across devices.

## The `.kitchensync/` Directory

Any directory with a `.kitchensync/` directory is an independent sync root. Each sync root has its own peer list, its own manifest, and operates independently — it knows nothing about parent or child directories that may also be sync roots.

This means subtrees can sync independently. A parent sync root walks into subdirectories that are also sync roots and syncs their files normally — it just skips all `.kitchensync/` directories.

Contents:

- `peers.conf` — list of peer URLs to sync with
- `manifest` — list of all known file paths on this device, one per line
- `reconcile_time` — timestamp of last successful walk
- `SNAP/` — tombstone files only (see "Tombstones" below)
- `XFER/<uuid>/<timestamp>/` — staging area for in-progress transfers (see "Transfer Staging" below)
- `BACK/` — files displaced by sync operations, organized by timestamp
- `kitchensync.sqlite` — SQLite database: config and logging only (see `quartz-lifecycle.md`)

## Tombstones (SNAP/)

When a file is deleted, a tombstone is created at `SNAP/<filename>` containing:

```json
{"del_time": "20260314T091523.847291Z"}
```

Tombstones are the only files in SNAP. There are no entries for live files — live file metadata (size, mtime) comes from the filesystem itself.

A tombstone records when a deletion was detected:
- **Watcher sees the deletion in real time** — `del_time` is the current time. Precise.
- **Walk detects the deletion** (file in manifest but not on disk) — `del_time` is set to `reconcile_time` from the previous run. This is conservative: the file could have been deleted any time after the last reconcile. If a peer modified it after our reconcile_time, the peer's modification wins. This biases toward preserving data.

Tombstones older than 6 months are deleted during walks.

## Manifest

The `manifest` file lists every file path known to exist on this device, one per line. It is updated at the end of each walk to reflect the current state of the filesystem.

The manifest's purpose is to detect deletions. When a walk finds a path in the manifest but not on disk, that file was deleted since the last run — a tombstone is created.

## Reconcile Time

The `reconcile_time` file contains a single timestamp — when the last walk successfully completed. It provides the conservative `del_time` for deletions detected during walks.

## Transfer Staging (XFER/)

All file transfers are staged through `.kitchensync/XFER/` on the destination device:

```
.kitchensync/XFER/<uuid>/<timestamp>/<filename>
```

The full transfer process (decide, transfer, recheck, swap) is described in `reconciliation.md`. The key property: the final swap is a rename on the same filesystem — near-atomic. A file is either fully present or not; there are no partially written files visible in the sync root.

Multiple threads may transfer the same file concurrently. Each gets its own UUID directory, so writes don't collide. Redundant work, never corruption.

XFER directories older than 2 days are deleted during walks.

## The `BACK/` Directory

When a sync operation would overwrite or remove a file, the existing file is moved to:

```
.kitchensync/BACK/<timestamp>/<filename>
```

No file is ever destroyed — displaced files are always recoverable from `BACK/`.

## Walk

A walk updates a device's metadata to match its filesystem. Same operation on every device; the only difference is I/O (local filesystem vs SFTP).

1. Read `manifest` → set of previously known paths.
2. Walk the filesystem → set of (path, size, mtime) for all files on disk.
3. Paths in manifest but not on disk → create tombstone with `del_time` = `reconcile_time`.
4. Paths on disk with existing tombstone → file resurrected, remove tombstone.
5. Write updated `manifest` (paths currently on disk).
6. Update `reconcile_time`.
7. Clean up: delete XFER directories older than 2 days, delete tombstones older than 6 months.

The walk also builds an **in-memory index** of the device's state: live files (path, size, mtime) from the directory listing, plus tombstones (path, del_time) from SNAP. This index is used by the compare step — no additional I/O needed.

### SFTP efficiency

For a remote peer with 50,000 files, the walk requires:
- Directory listing of the filesystem: ~4 MB (gives all file metadata for free)
- Directory listing of SNAP: negligible (only tombstones, typically few)
- Read `manifest`: ~2.5 MB (one file, 50,000 paths)
- Read tombstone files: negligible (few small files)
- Write updated `manifest`: ~2.5 MB (one file)
- Write `reconcile_time`: negligible (one small file)

**Total: ~9 MB.** No per-file reads or writes for live files.

## Compare

After all walks complete, the compare step diffs the in-memory indexes. For each peer, take the union of all paths across local and peer indexes and apply the decision rules from `reconciliation.md`:

- In local index only (live or tombstone) → enqueue on this peer's queue
- In peer index only (live or tombstone) → enqueue on this peer's queue
- In both but different (mtime, size, or deletion state) → enqueue on this peer's queue
- In both and matching → skip

This is entirely in-memory — zero I/O. The indexes were built during the walks.

## Peer Queues

Each peer has 10 queues. A queue entry is just a path — a lightweight hint meaning "this path probably needs reconciliation between local and this peer." The actual decision is made by the queue worker at processing time, using fresh data.

When a path needs to be enqueued for a peer, it goes to that peer's shortest queue.

### Queue workers

Each queue has a worker. The worker:

1. Dequeues a path.
2. Reads the current state of the path on both sides (stat the file, check for tombstone).
3. Applies the decision rules from `reconciliation.md`. If no action is needed (another worker already handled it, or state changed), skip.
4. If a transfer is needed, executes the 4-phase process (decide, transfer, recheck, swap).

A queue worker opens a connection to the peer when it dequeues its first item. The connection stays open while the queue is non-empty. When the queue drains, the connection is closed. Idle queues consume no resources.

### Connection efficiency

Ten queues per peer means up to 10 concurrent SFTP connections. For fast networks, all 10 may be active. For slow peers or small syncs, most queues stay empty and only one or two connections are used.

## Startup Sequence

### Once mode

1. **Quartz lifecycle** — init database, instance check, start HTTP server (see `quartz-lifecycle.md`).
2. **Walk all devices concurrently** — one thread per device (local + all reachable peers). Each walk updates the device's manifest, reconcile_time, and tombstones, and builds an in-memory index.
3. **Compare** — for each peer, diff the in-memory indexes and enqueue differing paths on that peer's queues.
4. **Drain queues** — wait for all peer queues to empty. Queue workers execute the 4-phase transfer for each path.
5. **Shutdown** — log `info` to database. Exit 0.

Once mode pushes all local changes to peers and pulls all changes from peers to local, but does not propagate changes between peers. For example, a file pulled from the NAS will not be pushed to the USB drive in the same run. A subsequent run (or watch mode) will complete the propagation. In watch mode, the watcher sees files arriving locally from any peer and automatically enqueues them for all other peers.

### Watch mode

Steps 1–4 are the same as once mode. Then:

5. **Steady state** — the filesystem watcher (started in step 1, see below) detects local changes. For each change, enqueue the path on each peer's shortest queue.
6. **Shutdown** — receive `POST /shutdown`. Stop the watcher. Drain remaining queues. Log `info` to database. Exit 0.

### Watch mode: the watcher

In watch mode, the filesystem watcher starts **immediately** after the quartz lifecycle completes — before the walks begin. Events accumulate in an unbounded FIFO queue. A watcher thread consumes from this queue: for each event, update the local manifest and tombstones as needed, then enqueue the path on each peer's shortest queue.

The walks (step 2) and queue draining (step 4) run concurrently with the watcher. Since all writes go through XFER staging, concurrent operations on the same path produce redundant work at worst, never corruption.

The watcher only monitors the local filesystem. There is no equivalent for peers — peers are only walked during startup. This is the one asymmetry between local and peer.

## Threading Model

- **Watcher thread** (watch mode only) — starts immediately. Consumes filesystem events, updates local manifest/tombstones, enqueues paths on peer queues.
- **Walk threads** — one per device (local + each reachable peer), running concurrently during startup. Each thread walks its device and builds an in-memory index.
- **Queue workers** — 10 per peer. Each worker dequeues a path, reads fresh state, applies decision rules, and executes the 4-phase transfer if needed. Opens a connection on first item, closes when queue drains.

Unreachable peers get no threads; they are skipped with a log entry and catch up on a future run. If a peer becomes unreachable mid-session, its queue workers log `error` and exit.
