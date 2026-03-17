# Reconciliation

How KitchenSync resolves differences between local and a peer.

## Queue-Based Evaluation

Queue entries contain only a relative path. The actual push/pull decision is made at dequeue time by comparing:
- Local database row for the path
- Peer database row for the path

This late-binding approach handles:
- State changes during transfer (another worker already handled it)
- Concurrent enqueues from multiple sources (watcher, local walker, peer walker)
- Network delays between enqueue and processing

## Inputs

For each path, there are three possible states on each side:

- **Live file** -- row exists with `del_time = NULL`, has `mod_time` and `byte_size`
- **Deleted** -- row exists with `del_time` set (tombstone)
- **Unknown** -- no row in database for this path

## Decision Rules

Compare local database against peer database for each path:

| Local   | Peer              | Action                                       |
| ------- | ----------------- | -------------------------------------------- |
| Live    | Live, same mtime  | Compare sizes (see below)                    |
| Live    | Live, local newer | Push to peer                                 |
| Live    | Live, peer newer  | Pull from peer                               |
| Live    | Deleted           | Compare `mod_time` vs `del_time` (see below) |
| Live    | Unknown           | Push to peer                                 |
| Deleted | Live              | Compare `del_time` vs `mod_time` (see below) |
| Deleted | Deleted           | No action                                    |
| Deleted | Unknown           | No action                                    |
| Unknown | Live              | Pull from peer                               |
| Unknown | Deleted           | No action                                    |
| Unknown | Unknown           | Not possible                                 |

### Timestamp Comparison Tolerance

When comparing timestamps, a tolerance of 2 seconds is applied. Two timestamps within 2 seconds of each other are considered equal. This accommodates FAT32's 2-second mtime resolution.

### Same Mtime, Different Size

When timestamps are equal (within tolerance) but sizes differ, the larger file wins. This biases toward preserving data. If sizes are also equal, no action is taken.

## Live vs Deleted

When one side has a live file and the other has a tombstone:

- **File is newer** (`mod_time > del_time`) -- the file wins. It is transferred to the deleted side. The tombstone is cleared (`del_time` set to NULL).
- **Deletion is newer** (`del_time > mod_time`) -- the deletion wins. The file is displaced to `BACK/`. A tombstone is created on that side.
- **Same timestamp** -- the file wins. This biases toward preserving data.

## Directory Reconciliation

Directories have `mod_time = NULL` and don't need timestamp-based conflict resolution. The rules are simpler:

| Local Dir | Peer Dir | Action                                    |
| --------- | -------- | ----------------------------------------- |
| Live      | Live     | No action                                 |
| Live      | Deleted  | Create on peer                            |
| Live      | Unknown  | Create on peer                            |
| Deleted   | Live     | Delete on peer (if empty, else skip)      |
| Deleted   | Deleted  | No action                                 |
| Deleted   | Unknown  | No action                                 |
| Unknown   | Live     | Create locally                            |
| Unknown   | Deleted  | No action                                 |

When deleting a directory on the destination, check if it's empty first. If not empty, skip -- the files inside will sync (or their deletions will propagate), and the directory deletion will succeed on a later pass.

Directory creation is implicit: parent directories are created as needed when syncing files. Explicit directory sync handles the case of intentionally empty directories.

## Push Operation

Transfer a file from local to peer:

1. **Read** local file
2. **Transfer** to peer's XFER staging: `peer:<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>`
3. **Recheck** peer state -- stat the destination on peer
4. If peer state changed and transfer is no longer warranted, abort and clean up XFER
5. **Displace** peer's existing file (if any) to `peer:.kitchensync/BACK/<timestamp>/`
6. **Swap** -- rename from XFER to final location on peer (same directory, instant)
7. **Cleanup** -- delete empty XFER directories
8. **Update** peer database row with new `mod_time`, `byte_size`, clear `del_time`

File content is streamed -- not loaded fully into memory. This allows transferring files larger than available RAM.

## Pull Operation

Transfer a file from peer to local:

1. **Read** peer file
2. **Transfer** to local XFER staging: `<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>`
3. **Recheck** local state -- stat the destination locally
4. If local state changed and transfer is no longer warranted, abort and clean up XFER
5. **Displace** local existing file (if any) to `.kitchensync/BACK/<timestamp>/`
6. **Swap** -- rename from XFER to final location (same directory, instant)
7. **Cleanup** -- delete empty XFER directories
8. **Update** local database row with new `mod_time`, `byte_size`, clear `del_time`

## Delete Propagation

When local has a tombstone and peer has a live file that is older:

1. **Displace** peer's file to `peer:.kitchensync/BACK/<timestamp>/`
2. **Update** peer database row: set `del_time`

The file is never truly deleted -- it goes to BACK/ and can be recovered.

## The Recheck Step

A transfer over SFTP can take minutes. During that time, the destination may change (another worker handled it, user edited the file, etc.). The recheck is cheap (one stat) compared to the transfer.

Recheck scenarios:
- **Destination unchanged** -- proceed with swap
- **Destination now newer than source** -- abort, delete XFER directory
- **Destination now matches source** -- abort, delete XFER directory (redundant transfer)
- **Destination deleted** -- proceed if source is live; abort if both deleted

## Displacing a File

When a file loses, it is moved to:

```
.kitchensync/BACK/<timestamp>/<filename>
```

The timestamp is the current time (when the displacement happens). Displaced files from both push and pull operations go to BACK/ on the side that lost.

When pushing to a peer, the peer's old file goes to `peer:.kitchensync/BACK/`. When pulling from a peer, the local old file goes to local `.kitchensync/BACK/`.

## Database Updates

After a successful transfer, the relevant database is updated:

**Push (local -> peer):**
- Peer database row updated to match local file's state

**Pull (peer -> local):**
- Local database row updated to match peer file's state

The database is updated **after** the transfer succeeds, not before. This ensures the database reflects actual state. If a transfer fails, the database remains accurate and the path will be re-discovered on the next rewalk. Updating optimistically (before transfer) would leave the database wrong on failure -- we'd think the peer has a file it doesn't, and the file would silently not sync.

The peer database (`.kitchensync/PEER/{name}.db`) is a local file, so updates are fast. No network I/O for database updates.

## Concurrent Workers

Multiple workers may process the same path concurrently (e.g., path enqueued by both watcher and walker). Each worker:
1. Gets its own XFER UUID -- writes don't collide
2. Does its own recheck -- detects if another worker already handled it
3. Aborts if the transfer is no longer warranted

Result: at most one worker's transfer completes; others detect this at recheck and abort. Redundant network I/O, never corruption.
