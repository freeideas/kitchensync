# Sync

## Command Line

```
kitchensync <config> [--canon <peer-name>] [-h|--help]
```

`<config>`: path to config file, `.kitchensync/` directory, or parent directory (see help.md).

`--canon <peer-name>`: named peer is authoritative for all decisions.

## Startup

1. Resolve config file path (see help.md)
2. Open database (WAL mode), run schema
3. Instance check — if another instance is using this database, print `Already running against <config-file-path>` and exit 0
4. Connect to all peers in parallel (skip unreachable, log warnings)
5. If `--canon` peer is unreachable, exit with error
6. The config must define at least two peers (validated at parse time). At runtime, require at least two reachable peers; with `--canon`, one reachable peer (the canon peer itself) is sufficient — the snapshot is updated from the canon peer's state so that when other peers come online, the sync can detect and propagate bidirectional changes rather than treating everything as new

## Run

1. Purge snapshot tombstones (rows where `deleted_time IS NOT NULL`) with `deleted_time` older than `tombstone-retention-days`. Also purge stale rows where `deleted_time IS NULL` and `last_seen` is older than `tombstone-retention-days` (or `last_seen` is NULL) — these are orphaned rows from entries that vanished from all peers. Purge expired log entries
2. Run combined-tree walk (see multi-tree-sync.md)
   - Directory creation and displacement (to BACK/) inline
   - File copies enqueued for concurrent execution
   - Snapshot updated during traversal
   - Per-peer concurrency limits enforced (see concurrency.md)
3. Wait for all enqueued file copies to complete
4. Disconnect all peers
5. Log completion, exit

## Operation Queue

File copies are enqueued during the combined-tree walk and executed concurrently, subject to per-peer connection limits (see concurrency.md). Directory creation and displacement to BACK/ run inline during the walk — both are same-filesystem operations that subsequent steps may depend on.

### File Copy

Each transfer is a `(src_peer, path, dst_peer, path)` pair. A transfer acquires one connection from the source peer's pool and one from the destination peer's pool before starting.

1. **Transfer** to XFER staging on destination: `<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>`
2. **If** the destination already has a file at the target path, **displace** it to `<file-parent>/.kitchensync/BACK/<timestamp>/<basename>`
3. **Swap** — rename from XFER to final path (same filesystem, atomic)
4. **Set mod_time** — set the destination file's modification time to the source file's mod_time
5. **Clean up** empty XFER directories

Content is streamed, not buffered entirely in memory. Each transfer spawns two concurrent tasks connected by a bounded channel: a reader task that reads chunks from the source and pushes them into the channel, and a writer task that pulls chunks and writes them to the destination. The reader and writer operate simultaneously — the channel provides backpressure (reader blocks when the channel is full, writer blocks when it is empty). A single-loop read-then-write pattern is not acceptable. On transfer failure, delete the XFER staging file/directory for that transfer before returning the connections to the pool.

### Displace to BACK

Each displacement is a `(peer, path)` pair executed inline during the combined-tree walk. The entry at `path` is renamed to `<parent>/.kitchensync/BACK/<timestamp>/<basename>`. A displaced directory is moved as a single rename, preserving its entire subtree.

## Logging

Every file copy and every deletion (displacement to BACK/) is logged at `info` level with a short format:

- Copy: `C <relative-path>`
- Delete: `X <relative-path>`

Logged once per decision, not per peer. This gives the user visible progress output (see quartz-lifecycle.md, KitchenSync exceptions).

## XFER Staging

Staged near the target for same-filesystem atomic rename. Inside `.kitchensync/` to stay hidden. UUID per transfer prevents collisions. Stale dirs cleaned after `xfer-cleanup-days` (default: 2).

## BACK Directory

No file is ever destroyed. Displaced entries are recoverable from BACK/. Cleaned after `back-retention-days` (default: 90).

## Peer Filesystem Abstraction

All sync logic — traversal, copy workers, XFER staging, BACK displacement, cleanup — operates through a single trait (interface) that both `file://` and `sftp://` implement. No protocol-specific code exists outside the trait implementations.

### Required Operations

| Operation                  | Description                                                       |
| -------------------------- | ----------------------------------------------------------------- |
| `list_dir(path)`           | List immediate children (name, is_dir, mod_time, byte_size). byte_size is file size in bytes for files, or −1 for directories |
| `stat(path)`               | Return mod_time, byte_size, is_dir; or "not found"                |
| `read_file(path)` → stream | Open file for streaming read                                      |
| `write_file(path, stream)` | Create/overwrite file from stream, creating parent dirs as needed |
| `rename(src, dst)`         | Same-filesystem rename (for XFER → final swap)                    |
| `delete_file(path)`        | Remove a file                                                     |
| `create_dir(path)`         | Create directory (and parents as needed)                          |
| `delete_dir(path)`         | Remove empty directory                                            |
| `set_mod_time(path, time)` | Set file/directory modification time                              |

### Error Semantics

All operations return the same error types regardless of transport: not found, permission denied, I/O error. The sync logic never matches on transport-specific errors. Network failures (connection drop, timeout) surface as I/O errors — the sync logic doesn't distinguish "disk read failed" from "SFTP channel died."

### Why This Matters

This is the sole mechanism that lets us test with `file://` and trust the result for `sftp://`. If any sync logic reaches around the trait to do something protocol-specific, that code is untested. The rule is absolute: if it touches a peer's filesystem, it goes through the trait.

## Errors

- **Config errors** (bad JSON5, unknown peer in `--canon`, missing file) → print to stdout, exit 1
- **Unreachable peer** → skip, log warning, continue with others
- **Transfer failure** → log, skip file (re-discovered next run)
- **Displacement failure** (cannot rename to BACK/) → log error, skip the displacement (file remains in place). If the displacement was part of a file copy sequence, the copy is also skipped (XFER staging file is cleaned up)
- **XFER staging failure** (cannot create staging directory or write staging file) → treat as transfer failure

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BACK/.
