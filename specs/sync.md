# Sync

## Command Line

```
kitchensync [--cfgdir <path>] [<url>...] [key=value...] [-h|--help]
```

No arguments, `-h`, or `--help`: print help and exit 0 (see help.md).

`<url>`: zero or more peer URLs or local paths. Each is a peer to sync. Bare paths are treated as `file://` URLs. A trailing `!` marks the URL as canon (e.g. `c:/photos!`). If no URLs are given, all groups in the config file are synced in parallel (each group runs independently). If the config file has no groups and no URLs are given, exit with error.

`key=value`: settings that apply to the current run and are persisted to the config file (see help.md, "Settings").

`--cfgdir <path>`: config directory. `<path>` is required when the flag is used. The config directory always ends with `.kitchensync/` — if `<path>` already ends with `.kitchensync/` or `.kitchensync`, it is used as-is (with a trailing `/` added if absent); otherwise `.kitchensync/` is appended. Examples: `--cfgdir ~` → `~/.kitchensync/`, `--cfgdir /mnt/usb` → `/mnt/usb/.kitchensync/`, `--cfgdir ~/.kitchensync` → `~/.kitchensync/`. Default (when `--cfgdir` is omitted entirely): `~/.kitchensync/`.

## Config Directory

Every run uses a config directory (default `~/.kitchensync/`) containing:
- `kitchensync-conf.json` — configuration (peer groups, settings)
- `kitchensync.db` — SQLite database (peer identity, snapshots)
- `quartz.db` — SQLite database (instance state, logs) — see quartz-lifecycle.md

## Peer Groups

A peer group is a set of peers that synchronize with each other. Groups are identified by their member URLs — specifying any URL from a group selects the entire group.

The config file is the source of truth for group membership. The database's `peer` and `peer_url` tables are reconciled against the config file on every startup (see database.md, Startup Reconciliation). The database does not track groups — only peer identity (URL → peer ID mapping). Edits to the config file take effect on the next run. Snapshot history is preserved through reconciliation — it is keyed by stable peer IDs, not by group structure.

CLI URLs are merged into the config file before reconciliation. The config file accumulates state across runs — URLs, peers, and settings are persisted, not just applied transiently.

## Startup

1. Resolve the config directory. Create it if it does not exist.
2. Load config file if it exists, merge CLI URLs and settings into it. Do not write yet.
3. Open databases (`quartz.db` and `kitchensync.db`), WAL mode, run schemas.
4. Instance check (via `quartz.db`) — if another instance is already running against this config directory, print `Already running` and exit 0.
5. Run peer identity reconciliation (see database.md, Startup Reconciliation). If reconciliation succeeds, write the merged config file. If reconciliation fails, exit with error — the original config file is unchanged.

Steps 6–9 apply independently to each group being synced. A group that fails validation is skipped with an error log; other groups proceed.

6. The group must have at least two peers.
7. If none of the group's peers have any snapshot data and no canon peer is designated, skip the group with error: bidirectional sync requires snapshot history or a canon peer. On a first run with no canon, suggest: "First sync? Mark the authoritative peer with a trailing !"
8. Connect to all peers in parallel (skip unreachable, log warnings). Auto-create the peer's root directory (and any missing parents) if it does not exist — for both `file://` and `sftp://` URLs. If directory creation fails, treat the peer as unreachable (log warning, try next fallback URL). If fewer than two peers are reachable (or fewer than one when a canon peer is designated), skip the group with error.
9. If canon peer is unreachable, skip the group with error.

## Canon Peer

A canon peer is authoritative — its state wins all conflicts.

- **`!` on the command line** (`c:/photos!`): designates the marked peer as canon and de-canons every other peer in the group, for this run only. Not persisted to the config file. A config-file `"canon": true` on another peer is overridden for the duration of the run. Intended for one-time bootstrapping.
- **`"canon": true` in the config file**: permanent canon. The user must edit the config file to set this.

At most one peer per group may be canon. Canon is required when no peer in the group has snapshot history (first run). Once snapshots exist, bidirectional sync works without a canon peer.

## Run

1. Purge snapshot tombstones older than `tombstone-retention-days`. Also purge stale rows where `deleted_time IS NULL` and `last_seen` is older than `tombstone-retention-days` (or `last_seen` is NULL). Purge expired log entries.
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

Each transfer is a `(src_peer, path, dst_peer, path)` pair. A transfer acquires one connection from the source peer's active URL pool and one from the destination peer's active URL pool before starting (see concurrency.md).

1. **Transfer** to XFER staging on destination: `<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>`
2. **If** the destination already has a file at the target path, **displace** it to `<file-parent>/.kitchensync/BACK/<timestamp>/<basename>`
3. **Swap** — rename from XFER to final path (same filesystem, atomic)
4. **Set mod_time** — set the destination file's modification time to the winning mod_time from the decision (not re-read from the source)
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

Staged near the target for same-filesystem atomic rename. Inside `.kitchensync/` to stay hidden. UUID per transfer prevents collisions. The `<timestamp>` in the path uses the format defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Stale dirs cleaned after `xfer-cleanup-days` (default: 2).

## BACK Directory

No file is ever destroyed. Displaced entries are recoverable from BACK/. The `<timestamp>` in the path uses the format defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Cleaned after `back-retention-days` (default: 90).

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

- **Config errors** (invalid settings, multiple canon peers, URLs from different groups, no canon and no snapshot history for group) → print to stdout, exit 1
- **Unreachable peer** → skip, log warning, continue with others
- **Transfer failure** → log, skip file (re-discovered next run)
- **Displacement failure** (cannot rename to BACK/) → log error, skip the displacement (file remains in place). If the displacement was part of a file copy sequence, the copy is also skipped (XFER staging file is cleaned up)
- **XFER staging failure** (cannot create staging directory or write staging file) → treat as transfer failure
- **Multi-group exit code** → exit 0 if all groups completed successfully, exit 1 if any group was skipped due to error

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BACK/.
