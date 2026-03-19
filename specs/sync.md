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
3. Instance check — if another instance is using this database, print `Already running against <config-file-path>` and exit
4. Connect to all peers in parallel (skip unreachable, log warnings)
5. If `--canon` peer is unreachable, exit with error
6. Require at least two reachable peers; with `--canon`, one reachable peer (the canon peer itself) is sufficient — the snapshot is updated from the canon peer's state so that when other peers come online, the sync can detect and propagate bidirectional changes rather than treating everything as new

## Run

1. Purge expired tombstones, log entries, stale XFER and BACK directories
2. Run multi-tree traversal (see multi-tree-sync.md)
   - Directories created/deleted inline
   - File copies enqueued for concurrent execution
   - Snapshot updated during traversal
   - Per-peer concurrency limits enforced (see concurrency.md)
3. Wait for all transfers to complete
4. Disconnect all peers
5. Log completion, exit

## File Copy

Each transfer is a `(src_peer, path, dst_peer, path)` pair. A transfer acquires one read slot on the source peer and one write slot on the destination peer before starting (see concurrency.md).

1. **Transfer** to XFER staging on destination: `<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>`
2. **Displace** existing file to `<file-parent>/.kitchensync/BACK/<timestamp>/<basename>`
3. **Swap** — rename from XFER to final path (same filesystem, atomic)
4. **Clean up** empty XFER directories

Content is streamed, not buffered in memory.

## Logging

Every file copy and every deletion (displacement to BACK/) is logged at `info` level with a short format:

- Copy: `C <relative-path>`
- Delete: `X <relative-path>`

Logged once per decision, not per peer. This gives the user visible progress output (see quartz-lifecycle.md, KitchenSync exceptions).

## XFER Staging

Staged near the target for same-filesystem atomic rename. Inside `.kitchensync/` to stay hidden. UUID per transfer prevents collisions. Stale dirs cleaned after `xfer-cleanup-days` (default: 2).

## BACK Directory

Displaced files and directories are moved to `<parent>/.kitchensync/BACK/<timestamp>/<basename>` on the peer that lost. A displaced directory is moved as a single rename, preserving its entire subtree. No file is ever destroyed. Cleaned after `back-retention-days` (default: 90).

## Peer Filesystem Abstraction

All sync logic — traversal, copy workers, XFER staging, BACK displacement, cleanup — operates through a single trait (interface) that both `file://` and `sftp://` implement. No protocol-specific code exists outside the trait implementations.

### Required Operations

| Operation                  | Description                                                       |
| -------------------------- | ----------------------------------------------------------------- |
| `list_dir(path)`           | List immediate children (name, is_dir, mod_time, byte_size)       |
| `stat(path)`               | Return mod_time, byte_size, is_dir; or "not found"                |
| `read_file(path)` → stream | Open file for streaming read                                      |
| `write_file(path, stream)` | Create/overwrite file from stream, creating parent dirs as needed |
| `rename(src, dst)`         | Same-filesystem rename (for XFER → final swap)                    |
| `delete_file(path)`        | Remove a file                                                     |
| `create_dir(path)`         | Create directory (and parents as needed)                          |
| `delete_dir(path)`         | Remove empty directory                                            |

### Error Semantics

All operations return the same error types regardless of transport: not found, permission denied, I/O error. The sync logic never matches on transport-specific errors. Network failures (connection drop, timeout) surface as I/O errors — the sync logic doesn't distinguish "disk read failed" from "SFTP channel died."

### Why This Matters

This is the sole mechanism that lets us test with `file://` and trust the result for `sftp://`. If any sync logic reaches around the trait to do something protocol-specific, that code is untested. The rule is absolute: if it touches a peer's filesystem, it goes through the trait.

## Errors

- **Config errors** (bad JSON5, unknown peer in `--canon`, missing file) → print to stdout, exit
- **Unreachable peer** → skip, log warning, continue with others
- **Transfer failure** → log, skip file (re-discovered next run)

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BACK/.
