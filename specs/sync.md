# Sync

How KitchenSync synchronizes files across devices.

## The `.kitchensync/` Directory

Any directory with a `.kitchensync/` directory can be an independent sync root. Each sync root has its own peer list, its own database, and operates independently.

Contents:

| Path                | Purpose                                          |
| ------------------- | ------------------------------------------------ |
| `kitchensync.db`    | Local snapshot database (see `database.md`)      |
| `peers.conf`        | Peer configuration (see `peers.md`)              |
| `PEER/`             | Peer databases with queues (persist across runs) |
| `BACK/<timestamp>/` | Displaced files, organized by timestamp          |

Note: XFER staging directories are created in `.kitchensync/XFER/` directories throughout the tree (adjacent to target files), not just at the sync root. See "Transfer Staging" below.

## Local Database

The local database (`.kitchensync/kitchensync.db`) contains the snapshot table -- the known state of the local filesystem. This enables fast startup:

1. On startup, compare the database to the filesystem
2. Any differences are immediately detected -- no need to walk peers first
3. Changed paths are enqueued to all peers within seconds

See `database.md` for schema details.

## Peer Databases

Each configured peer has a database at `.kitchensync/PEER/{peer-name}.db` containing:
- **snapshot** table -- the peer's filesystem state, with tombstones for deleted files
- **queue** table -- paths awaiting sync with this peer
- **config** table -- peer-specific config (`last_walk_time`)

Queue entries persist across runs and disconnections, enabling fast sync when a peer reconnects. At startup, peer databases for peers not listed in `peers.conf` are deleted -- this cleans up after peer removal. The snapshot table persists across connections, enabling deletion detection (see Peer Walker).

See `database.md` for schema details.

## Tombstones

When a file is deleted, its snapshot row is updated: `del_time` is set to the deletion timestamp. The row remains as a tombstone.

Tombstone timing:
- **Watcher detects deletion in real time** -- `del_time` is the current time. Precise.
- **Walker detects deletion** (in database but not on disk) -- `del_time` is set to the `last_walk_time` from the config table. This is conservative: the file could have been deleted any time after the last walk. If a peer modified it after our last walk, the peer's modification wins.

The `last_walk_time` is stored in the config table and updated at the end of each successful local walk.

### Resurrection

If a deleted file reappears (exists on disk but has `del_time` set in the database), compare `mod_time` to `del_time`:
- `mod_time > del_time` -- the file wins. Clear `del_time`, update the row, enqueue to peers.
- `mod_time <= del_time` -- the deletion wins. The file appeared from somewhere stale (e.g., restored from old backup). Delete it again, move to BACK/.

Tombstones older than `tombstone-retention-days` (default: 180, ~6 months) are deleted during walks. Why 6 months default? Long enough for occasionally-connected peers (USB drives, laptops) to sync before the deletion record expires; short enough to not accumulate forever.

## Transfer Staging (XFER/)

File transfers are staged in an XFER directory on the destination device. The XFER directory is created inside a `.kitchensync/` directory in the **target file's parent directory**:

```
<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>
```

For example, transferring `docs/notes/readme.txt` stages to:
```
docs/notes/.kitchensync/XFER/20260314T091523.847291Z/a1b2c3d4/readme.txt
```

The XFER directory contains only the file's basename, not a path -- the directory structure is already encoded by the XFER directory's location.

Why near the target? If the sync root spans multiple mounted filesystems, placing XFER in the same directory as the target ensures the final rename is same-filesystem -- instant rather than copy+delete. The sync root's `.kitchensync/` might be on a different device than the target file.

Why inside `.kitchensync/`? Keeps staging directories hidden from users. No visible pollution in their file tree.

Why timestamp in path? Makes it trivial to identify and clean up stale transfers -- just compare the directory name to the current time.

The transfer process:
1. **Transfer** -- copy file to XFER staging in target parent's `.kitchensync/XFER/`
2. **Recheck** -- stat the destination; if state changed, re-evaluate
3. **Swap** -- displace existing file to BACK/, rename from XFER to final location
4. **Cleanup** -- delete the empty XFER directories (uuid, then timestamp if empty)

The swap is a rename on the same filesystem -- instant and atomic. Files are either fully present or not; no partial writes visible in the sync root.

Multiple threads may transfer the same file concurrently. Each gets its own UUID directory, so writes don't collide.

Stale XFER directories (from crashes) older than `xfer-cleanup-days` (default: 2) are deleted during walks. Why 2 days default? Long enough that a slow transfer won't be interrupted; short enough to clean up crash debris promptly. The walker scans for `.kitchensync/XFER/` directories throughout the tree, not just at the sync root.

When walking a peer, the walker also deletes stale `.kitchensync/XFER/` directories older than `xfer-cleanup-days` on the peer, handling crash debris from our push operations.

File permissions are not synchronized. New files use the destination system's default permissions (umask-based).

If the destination file is read-only, KitchenSync clears the read-only attribute before displacement. On failure, the transfer is skipped and logged as an error.

## The `BACK/` Directory

When a sync operation would overwrite or remove a file, the existing file is moved to:

```
.kitchensync/BACK/<timestamp>/<filename>
```

No file is ever destroyed -- displaced files are always recoverable.

BACK directories older than `back-retention-days` (default: 90) are deleted during walks. Why 90 days default? Long enough to recover from accidental sync conflicts or user mistakes; short enough to reclaim disk space from obsolete backups.

## Peer Queues

Each peer has a queue stored in its SQLite database (`.kitchensync/PEER/{name}.db`). A queue entry is just a relative path -- a lightweight hint meaning "this path probably needs sync between local and this peer."

Queue entries persist across runs and disconnections. When a peer reconnects after being offline, all accumulated changes are ready to sync immediately.

### Queue Characteristics

- **Deduplicated by path** -- same path enqueued twice results in one entry (with refreshed timestamp)
- **Capped at 10,000 entries** -- when full, oldest entries are dropped to make room
- **Recent-first priority** -- newer changes get the "fast path"; older changes that overflow are caught by the peer walk

The queue is an optimization, not the source of truth. The peer walk catches everything, including paths that overflowed the queue.

## Connection Manager

Each peer has a dedicated connection manager thread that handles the connection lifecycle:

```
loop forever:
    if queue is non-empty:
        connect (try URLs in order, retry every `retry-interval` seconds on failure)
        spawn `workers-per-peer` worker threads → drain queue
        wait for queue to empty (or connection drop)
        disconnect
    else if time since last_walk_time > rewalk_after_minutes:
        connect (try URLs in order)
        walk peer filesystem → update snapshot table
        record last_walk_time
        drain any queue entries discovered by walk
        disconnect
    else:
        sleep briefly, check again
```

On first iteration, `last_walk_time` is unset, so `time since last_walk_time > rewalk_after_minutes` is true -- triggering an immediate walk. This catches any changes that occurred while disconnected. Periodic re-walks (controlled by `rewalk-after-minutes`, default 12 hours) catch external changes on peers that don't run KitchenSync.

Why `workers-per-peer` workers (default: 10)? This allows concurrent transfers per peer, saturating fast network links. All workers share the SSH connection (via separate SFTP channels).

### No Idle Connections

Connections only open when there's work to do (queue non-empty or time to re-walk). When the queue drains, the connection closes. This is friendly to:
- Peers that sleep (NAS, laptops)
- Networks with connection limits
- Long-running watch mode sessions

### Worker Threads

When a connection is established, `workers-per-peer` (default: 10) worker threads are spawned. Each worker:

1. Dequeues a path from the SQLite queue
2. Looks up the path in local database and peer database
3. Applies decision rules from `reconciliation.md`
4. If action needed, executes push or pull through XFER staging
5. Updates both databases after successful transfer
6. Repeats until queue is empty

Workers exit when the queue is empty. The connection manager waits for all workers to finish, then closes the connection.

If a transfer fails (network error, permission denied, etc.), the worker logs the error and continues to the next queue item. The failed path is not re-enqueued -- it will be re-discovered on the next periodic rewalk.

## Startup Sequence

### Watch Mode (default)

1. **Init** -- open local database, instance check
2. **Start watcher** -- immediately, before walks begin. Why first? To catch any filesystem changes that happen during the walks; otherwise those changes could be missed.
3. **Start local walker** -- compares database to filesystem, enqueues differences to all peers' SQLite queues
4. **Start connection managers** -- one thread per configured peer, each runs independently
5. **Steady state** -- watcher detects changes, enqueues to all peers; connection managers handle sync
6. **Shutdown** -- receive `POST /shutdown`, stop watcher, wait for connection managers to drain queues, exit

### Once Mode (`--once`)

1. **Init** -- open local database, instance check
2. **Start local walker** -- compares database to filesystem, enqueues to all peers
3. **Start connection managers** -- one thread per configured peer
4. **Wait for completion** -- all connection managers must: connect, walk peer, drain queue, disconnect
5. **Shutdown** -- exit 0

In once mode, changes detected by peer walkers are enqueued to all peers (not just the detecting peer). This propagates changes between peers in a single run. Connection managers exit after one cycle instead of looping.

## Local Walker

The local walker compares the database to the filesystem:

**Phase 1: Walk the filesystem**
- For each file/directory on disk, check the database
- If not in database (new): insert row, enqueue to all peers
- If in database but changed (mod_time or size): update row, enqueue to all peers
- If in database with `del_time` set (resurrection): compare `mod_time` to `del_time`
  - If `mod_time > del_time`: file wins. Clear `del_time`, update row, enqueue to all peers.
  - If `mod_time <= del_time`: deletion wins. Delete the file, move to BACK/. (File appeared from stale source.)

**Phase 2: Walk the database**
- For each row without `del_time`, check the filesystem
- If not on disk (deleted): set `del_time` to `last_walk_time`, enqueue to all peers

**Phase 3: Update last_walk_time**
- Store current timestamp in config table as `last_walk_time`

This detects all local changes since the last run and fans them out to all peers immediately -- no peer walks needed first.

## Peer Walker

The peer walker runs inside the connection manager, immediately after establishing a connection. It mirrors the local walker's logic, updating the peer snapshot incrementally rather than clearing it.

**Phase A: Walk the remote filesystem**
- Walk the peer's filesystem over SFTP
- For each file on peer, check the peer snapshot table:
  - If not in snapshot (new): insert row, enqueue to this peer's queue
  - If in snapshot but changed (mod_time or size): update row, enqueue to this peer's queue
  - If in snapshot with `del_time` set (resurrection): compare `mod_time` to `del_time`
    - If `mod_time > del_time`: file wins. Clear `del_time`, update row, enqueue.
    - If `mod_time <= del_time`: deletion wins. Skip (deletion will propagate to peer).
- In `--once` mode: also enqueue to all other peers' queues

**Phase B: Walk the peer snapshot**
- For each row in peer snapshot without `del_time`, check the peer filesystem
- If not on peer (deleted): set `del_time` to `last_walk_time`, enqueue to this peer's queue
- In `--once` mode: also enqueue to all other peers' queues

**Phase C: Update last_walk_time**
- Store current timestamp in peer database's config table as `last_walk_time`

This catches:
- Files that exist on the peer but not locally (or are different)
- Files that exist locally but not on the peer
- Files deleted on the peer since last walk (tombstones with accurate `del_time`)

## Threading Model

- **Watcher thread** (watch mode only) -- starts immediately, updates local database, enqueues paths to all peers' SQLite queues
- **Local walker thread** -- compares database to filesystem at startup, enqueues differences to all peers
- **Connection manager threads** -- one per configured peer, handles connect/walk/drain/disconnect cycle
- **Worker threads** -- `workers-per-peer` (default: 10) per peer, spawned by connection manager when connected, drain the queue

The watcher and local walker start immediately at startup. Connection managers start concurrently and independently retry connections every `retry-interval` seconds until successful. When a connection succeeds, the peer walk runs, then workers drain the queue, then the connection closes.

## Configuration Watching

KitchenSync watches `.kitchensync/peers.conf` for changes. When the file is modified:

1. Wait 500ms after the last modification (debounce)
2. Log the configuration reload
3. Gracefully shut down (same sequence as `POST /shutdown`)
4. Re-execute with the original command-line arguments

This applies to both watch mode and `--once` mode. The effect is identical to manually stopping KitchenSync, editing peers.conf, and restarting. Use case: you start `--once` and realize it's syncing to an unwanted slow device -- edit peers.conf to remove it and KitchenSync restarts without that peer.

Why debounce? Text editors often write files in multiple steps (write temp, rename, or multiple rapid saves). Waiting 500ms after the last change avoids restarting on partial writes.

Why restart instead of hot-reload? Simpler implementation, no edge cases around mid-sync config changes, and the existing design (persistent queues, XFER cleanup) already handles restarts gracefully.
