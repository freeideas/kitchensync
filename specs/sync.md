# Sync

## Command Line

```
kitchensync [options] <peer> <peer> [<peer>...]
```

No arguments, `-h`, `--help`, or `/?`: print help and exit 0 (see help.md).

### Peers

Each `<peer>` argument is a URL or local path identifying a sync target. Bare paths (no scheme) are treated as `file://` URLs. At least two peers are required.

Prefixes:
- **`+`** — canon peer. Its state wins all conflicts. Example: `+c:/photos`
- **`-`** — subordinate peer. Does not contribute to decisions; receives the group's outcome. Example: `-/mnt/usb/photos`
- **(none)** — normal bidirectional peer. Contributes and receives based on snapshot history.

At most one `+` peer per run. Multiple `-` peers are allowed.

### Fallback URLs

Square brackets group multiple URLs into a single peer (different network paths to the same data). URLs are tried in order; the first that connects wins.

```
kitchensync +[sftp://192.168.1.50/photos,sftp://nas.vpn/photos] /local/photos
```

The `+`/`-` prefix goes on the bracket, not on individual URLs inside.

### Per-URL Settings

Query-string parameters on a URL override global settings for that URL's connection:

```
"sftp://host/path?mc=5&ct=60"
```

| Param | Meaning            | Global flag |
| ----- | ------------------ | ----------- |
| `mc`  | Max connections    | `--mc`      |
| `ct`  | Connection timeout | `--ct`      |

Query-string parameters are stripped during URL normalization — they are not part of the URL's identity.

### Global Options

| Flag   | Default | Meaning                             |
| ------ | ------- | ----------------------------------- |
| `--mc` | 10      | Max concurrent connections per URL  |
| `--ct` | 30      | Seconds for SSH handshake timeout   |
| `-vl`  | `info`  | Verbosity level (error, info, debug, trace) |
| `--xd` | 2       | Delete stale TMP staging after N days |
| `--bd` | 90      | Delete displaced files (BAK/) after N days |
| `--td` | 180     | Forget deletion records after N days |

### URL Schemes

| Form                                 | Meaning                           |
| ------------------------------------ | --------------------------------- |
| `/path` or `c:\path` or `./relative` | Local path (becomes `file://`)    |
| `sftp://user@host/path`              | Remote over SSH (port 22)         |
| `sftp://user@host:port/path`         | Non-standard SSH port             |
| `sftp://user:password@host/path`     | Inline password (prefer SSH keys) |

Percent-encode special characters in SFTP passwords (`@` → `%40`, `:` → `%3A`). SFTP paths are absolute from filesystem root.

### Authentication (fallback chain)

1. Inline password from URL
2. SSH agent (`SSH_AUTH_SOCK`)
3. `~/.ssh/id_ed25519`
4. `~/.ssh/id_ecdsa`
5. `~/.ssh/id_rsa`

Host keys verified via `~/.ssh/known_hosts`. Unknown hosts rejected.

## Canon Peer (`+`)

A canon peer is authoritative — its state wins all conflicts unconditionally.

Canon is required when no peer in the group has snapshot history (first run). Without snapshots, there's no history to distinguish new files from deleted files, so one peer must be the source of truth. Once snapshots exist, bidirectional sync works without a canon peer.

On a first run with no canon, print: `First sync? Mark the authoritative peer with a leading +`

## Subordinate Peer (`-`)

A subordinate peer does not contribute to decisions. During the decision phase, its files are invisible — decisions are made using only normal and canon peers. After decisions are made, the subordinate peer is made to match the outcome: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it.

Any peer without a snapshot (no `.kitchensync/snapshot.db`) is automatically treated as subordinate, unless it is the canon peer (`+`). The `-` prefix is redundant for snapshotless peers but harmless. This means new peers always receive the group's state without influencing decisions.

A subordinate peer's snapshot is still downloaded and updated. On future runs (without `-`), the peer participates normally using its snapshot history.

## Startup

1. Parse command line. Validate: at least two peers, at most one `+` peer, no unrecognized flags, and all option values are valid (e.g., `--mc` and `--ct` are positive integers, `-vl` is one of `error`/`info`/`debug`/`trace`). On any validation error, print the error message followed by the help text and exit 1.
2. Connect to all peers in parallel. Auto-create the peer's root directory (and any missing parents) if it does not exist — for both `file://` and `sftp://` URLs. For peers with fallback URLs (bracket syntax), try URLs in order; first that connects wins. Skip unreachable peers with a warning. If directory creation fails, treat the peer as unreachable (try next fallback URL).
3. If fewer than two peers are reachable, exit with error.
4. If canon peer (`+`) is unreachable, exit with error.
5. Download each peer's `.kitchensync/snapshot.db` to a local temp directory (`{tmp}/{uuid}/snapshot.db`). If a peer has no `snapshot.db`, create a new empty one locally.
6. Peers whose `.kitchensync/snapshot.db` did not exist on disk (i.e., a new empty database was created in step 5) are automatically treated as subordinate. If no peer has any snapshot data and no canon peer (`+`) is designated, exit with error: there must be at least one contributing peer (suggest `+`).
7. If no contributing (non-subordinate) peer is reachable after auto-subordination, exit with error: `No contributing peer reachable — cannot make sync decisions`

## Run

1. Purge snapshot tombstones older than `--td` days. Also purge stale rows where `deleted_time IS NULL` and `last_seen` is older than `--td` days (or `last_seen` is NULL).
2. Run combined-tree walk (see multi-tree-sync.md)
   - Directory creation and displacement (to BAK/) inline
   - File copies enqueued for concurrent execution
   - Snapshot updated during traversal
   - Per-peer concurrency limits enforced (see concurrency.md)
3. Wait for all enqueued file copies to complete
4. Write updated snapshots back to peers atomically: upload as `snapshot-new.db`, rename to `snapshot.db` (see database.md)
5. Disconnect all peers
6. Log completion, exit 0

## Operation Queue

File copies are enqueued during the combined-tree walk and executed concurrently, subject to per-peer connection limits (see concurrency.md). Directory creation and displacement to BAK/ run inline during the walk — both are same-filesystem operations that subsequent steps may depend on.

### File Copy

Each transfer is a `(src_peer, path, dst_peer, path)` pair. A transfer acquires one connection from the source peer's active URL pool and one from the destination peer's active URL pool before starting (see concurrency.md).

1. **Transfer** to TMP staging on destination: `<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>`
2. **If** the destination already has a file at the target path, **displace** it to `<file-parent>/.kitchensync/BAK/<timestamp>/<basename>`
3. **Swap** — rename from TMP to final path (same filesystem, atomic)
4. **Set mod_time** — set the destination file's modification time to the winning mod_time from the decision (not re-read from the source)
5. **Clean up** empty TMP directories

Content is streamed, not buffered entirely in memory. Each transfer spawns two concurrent tasks connected by a bounded channel: a reader task that reads chunks from the source and pushes them into the channel, and a writer task that pulls chunks and writes them to the destination. The reader and writer operate simultaneously — the channel provides backpressure (reader blocks when the channel is full, writer blocks when it is empty). A single-loop read-then-write pattern is not acceptable. On transfer failure, delete the TMP staging file/directory for that transfer before returning the connections to the pool.

### Displace to BAK

Each displacement is a `(peer, path)` pair executed inline during the combined-tree walk. The entry at `path` is renamed to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. A displaced directory is moved as a single rename, preserving its entire subtree.

## Logging

All output goes to stdout.

Every file copy and every deletion (displacement to BAK/) is logged at `info` level with a short format:

- Copy: `C <relative-path>`
- Delete: `X <relative-path>`

Logged once per decision, not per peer. This gives the user visible progress output.

## TMP Staging

Staged near the target for same-filesystem atomic rename. Inside `.kitchensync/` to stay hidden. UUID per transfer prevents collisions. The `<timestamp>` in the path uses the format defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Stale dirs cleaned after `--xd` days (default: 2).

## BAK Directory

No file is ever destroyed. Displaced entries are recoverable from BAK/. The `<timestamp>` in the path uses the format defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Cleaned after `--bd` days (default: 90).

## Peer Filesystem Abstraction

All sync logic — traversal, copy workers, TMP staging, BAK displacement, cleanup — operates through a single trait (interface) that both `file://` and `sftp://` implement. No protocol-specific code exists outside the trait implementations.

### Required Operations

| Operation                  | Description                                                       |
| -------------------------- | ----------------------------------------------------------------- |
| `list_dir(path)`           | List immediate children (name, is_dir, mod_time, byte_size). byte_size is file size in bytes for files, or −1 for directories |
| `stat(path)`               | Return mod_time, byte_size, is_dir; or "not found"                |
| `read_file(path)` → stream | Open file for streaming read                                      |
| `write_file(path, stream)` | Create/overwrite file from stream, creating parent dirs as needed |
| `rename(src, dst)`         | Same-filesystem rename (for TMP → final swap)                    |
| `delete_file(path)`        | Remove a file                                                     |
| `create_dir(path)`         | Create directory (and parents as needed)                          |
| `delete_dir(path)`         | Remove empty directory                                            |
| `set_mod_time(path, time)` | Set file/directory modification time                              |

`list_dir` returns only regular files and directories. Symbolic links, special files (devices, FIFOs, sockets), and any other non-regular entry types are silently omitted by the implementation. The same applies to `stat`: if the path is a symlink or special file, return "not found."

### Error Semantics

All operations return the same error types regardless of transport: not found, permission denied, I/O error. The sync logic never matches on transport-specific errors. Network failures (connection drop, timeout) surface as I/O errors — the sync logic doesn't distinguish "disk read failed" from "SFTP channel died."

### Why This Matters

This is the sole mechanism that lets us test with `file://` and trust the result for `sftp://`. If any sync logic reaches around the trait to do something protocol-specific, that code is untested. The rule is absolute: if it touches a peer's filesystem, it goes through the trait.

## Errors

- **Argument errors** (fewer than two peers, multiple `+` peers, invalid settings) → print to stdout, exit 1
- **No snapshots and no canon** → print suggestion (`+`), exit 1
- **Unreachable peer** → skip, log warning, continue with others
- **Canon peer unreachable** → exit with error
- **Fewer than two reachable peers** → exit with error
- **Transfer failure** → log, skip file (re-discovered next run)
- **Displacement failure** (cannot rename to BAK/) → log error, skip the displacement (file remains in place). If the displacement was part of a file copy sequence, the copy is also skipped (TMP staging file is cleaned up)
- **TMP staging failure** (cannot create staging directory or write staging file) → treat as transfer failure
- **Snapshot upload failure** → log error (peer's snapshot will be stale on next run, leading to redundant but correct copies)

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BAK/.
