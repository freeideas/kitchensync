# Sync

## Command Line

```
java -jar kitchensync.jar [options] <peer> <peer> [<peer>...]
```

No arguments, `-h`, `--help`, or `/?`: print help and exit 0 (see help.md).

### Peers

Each `<peer>` argument is a URL or local path identifying a sync target. Bare paths (no scheme) are treated as `file://` URLs. At least two peers are required.

Prefixes:
- **`+`** - canon peer. Its state wins all conflicts. Example: `+c:/photos`
- **`-`** - subordinate peer. Does not contribute to decisions; receives the group's outcome. Example: `-/mnt/usb/photos`
- **(none)** - normal bidirectional peer. Contributes and receives based on snapshot history.

At most one `+` peer per run. Multiple `-` peers are allowed.

### Fallback URLs

Square brackets group multiple URLs into a single peer (different network paths to the same data). URLs are tried in order; the first that connects wins.

```
java -jar kitchensync.jar +[sftp://192.168.1.50/photos,sftp://nas.vpn/photos] /local/photos
```

The `+`/`-` prefix goes on the bracket, not on individual URLs inside.

### Per-URL Settings

Query-string parameters on a URL override global settings (see concurrency.md for which settings apply per-connection vs per-pool):

```
"sftp://host/path?mc=5&ct=60"
```

| Param | Meaning              | Global flag |
| ----- | -------------------- | ----------- |
| `mc`  | Max SFTP connections | `--mc`      |
| `ct`  | Connection timeout   | `--ct`      |
| `ka`  | Idle keep-alive TTL  | `--ka`      |

Query-string parameters are stripped during URL normalization - they are not part of the URL's identity.

### Command-Line Excludes

`-x <relative-path>` excludes one path from scanning, decisions, copying,
deletion, displacement, and snapshot updates. The flag is repeatable.

Exclude paths are slash-separated relative paths in the same format KitchenSync
prints in stdout progress lines:

- no leading `/`;
- no trailing `/`;
- no `\` separators;
- no empty, `.`, or `..` path segments;
- no NUL characters.

If the excluded path is a file, only that file is skipped. If it is a directory,
the directory and all descendants are skipped. Excluded entries are treated as
if they do not exist for this run. Existing excluded files or directories on any
peer are left untouched, and existing snapshot rows for excluded paths are not
consulted or updated during the run.

Command-line excludes are stronger than `.syncignore`: a `.syncignore` negation
cannot re-include a path excluded by `-x`. Excluding `.syncignore` itself
prevents that ignore file from being resolved at that directory.

### Global Options

| Flag   | Default | Meaning                                |
| ------ | ------- | -------------------------------------- |
| `--mc` | 10      | Max concurrent transfers/connections    |
| `--ct` | 30      | Seconds for SSH handshake timeout      |
| `--ka` | 30      | SFTP idle keep-alive TTL (seconds)     |
| `-vl`  | `info`  | Verbosity level (error, info, debug, trace) |
| `-x`   | -       | Exclude a relative path from scanning and copying; repeatable |
| `--dir-status` | 10 | Seconds of quiet stdout before directory status is logged; 0 disables |
| `--xd` | 2       | Delete stale TMP staging after N days  |
| `--bd` | 90      | Delete displaced files (BAK/) after N days |
| `--td` | 180     | Forget deletion records after N days   |

### URL Schemes

| Form                                 | Meaning                           |
| ------------------------------------ | --------------------------------- |
| `/path` or `c:\path` or `./relative` | Local path (becomes `file://`)    |
| `sftp://user@host/path`              | Remote over SSH (port 22)         |
| `sftp://user@host:port/path`         | Non-standard SSH port             |
| `sftp://host/path`                   | Remote over SSH, current OS user  |
| `sftp://user:password@host/path`     | Inline password (prefer SSH keys) |

Percent-encode special characters in SFTP passwords (`@` -> `%40`, `:` -> `%3A`). SFTP paths are absolute from filesystem root.

### Authentication (fallback chain)

1. Inline password from URL
2. SSH agent (`SSH_AUTH_SOCK`)
3. `~/.ssh/id_ed25519`
4. `~/.ssh/id_ecdsa`
5. `~/.ssh/id_rsa`

Host keys verified via `~/.ssh/known_hosts`. Unknown hosts rejected.

## Canon Peer (`+`)

A canon peer is authoritative - its state wins all conflicts unconditionally.

Canon is required when no peer in the group has snapshot history (first run). Without snapshots, there's no history to distinguish new files from deleted files, so one peer must be the source of truth. Once snapshots exist, bidirectional sync works without a canon peer.

On a first run with no canon, print: `First sync? Mark the authoritative peer with a leading +`

## Subordinate Peer (`-`)

A subordinate peer does not contribute to decisions. During the decision phase, its files are invisible - decisions are made using only normal and canon peers. After decisions are made, the subordinate peer is made to match the outcome: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it.

Any peer without a snapshot (no `.kitchensync/snapshot.db`) is automatically treated as subordinate, unless it is the canon peer (`+`). The `-` prefix is redundant for snapshotless peers but harmless. This means new peers always receive the group's state without influencing decisions.

A subordinate peer's snapshot is still downloaded and updated. On future runs (without `-`), the peer participates normally using its snapshot history.

## Startup

1. Parse command line. A help invocation is any of: `-h`, `--help`, `/?`, or no arguments at all; help invocations print the help text to stdout and exit 0 (see `help.md`). For non-help invocations, validate: at least two peers, at most one `+` peer, no unrecognized flags, and all option values are valid (e.g., `--mc`, `--ct`, `--ka`, `--xd`, `--bd`, and `--td` are positive integers; `--dir-status` is a non-negative integer; `-vl` is one of `error`/`info`/`debug`/`trace`; every `-x` path is a valid relative slash path). On any validation error, print the error message followed by the help text and exit 1.
2. Connect to all peers in parallel. Auto-create the peer's root directory (and any missing parents) if it does not exist - for both `file://` and `sftp://` URLs. For peers with fallback URLs (bracket syntax), try URLs in order; first that connects wins. Skip unreachable peers with an error-level diagnostic. If directory creation fails, treat the peer as unreachable (try next fallback URL).
3. If fewer than two peers are reachable, exit with error.
4. If canon peer (`+`) is unreachable, exit with error.
5. Download each peer's `.kitchensync/snapshot.db` to a local temp directory (`{tmp}/{uuid}/snapshot.db`). If a peer has no `snapshot.db` (transport returns 'not found'), create a new empty one locally. If the download fails with any other error (I/O error, permission denied), treat the peer as unreachable: log an error-level diagnostic and exclude it from the reachable set, then re-evaluate steps 3-4 against the updated set and exit with the corresponding error if either check now fails.
6. Peers whose `.kitchensync/snapshot.db` did not exist on disk (i.e., a new empty database was created in step 5) are automatically treated as subordinate unless they are the canon peer (`+`). If no peer has any snapshot data and no canon peer (`+`) is designated, print `First sync? Mark the authoritative peer with a leading +` and exit 1.
7. If no contributing (non-subordinate) peer is reachable after auto-subordination, exit with error: `No contributing peer reachable - cannot make sync decisions`

## Run

1. Purge snapshot tombstones older than `--td` days. Also purge stale rows where `deleted_time IS NULL` and `last_seen` is older than `--td` days (or `last_seen` is NULL).
2. Run combined-tree walk (see multi-tree-sync.md)
   - Directory creation and displacement (to BAK/) inline
   - File copies enqueued for concurrent execution
   - Snapshot updated during traversal
   - Per-peer concurrency limits enforced (see concurrency.md)
3. Wait for all enqueued file copies to complete
4. Write updated snapshots back to peers using TMP staging: upload to `.kitchensync/TMP/<timestamp>/<uuid>/snapshot.db`, rename to `.kitchensync/snapshot.db` (see database.md). Failed uploads leave staging files that are cleaned up after `--xd` days like any other stale TMP file
5. Disconnect all peers
6. Log completion, exit 0

## Operation Queue

File copies are enqueued during the combined-tree walk and executed concurrently, subject to per-peer connection limits (see concurrency.md). Directory creation and displacement to BAK/ run inline during the walk - both are same-filesystem operations that subsequent steps may depend on.

### Rename Compatibility

KitchenSync must not assume that transport `rename(src, dst)` overwrites an
existing destination. This matters for SFTP servers that reject rename when
`dst` already exists.

User data replacement is a BAK operation, not a delete operation. When a copied
file would replace an existing file or directory, KitchenSync must first move
the existing destination to BAK, then rename the fully staged replacement into
place. If moving the existing destination to BAK fails, the original destination
must remain in place, the staged TMP file must be cleaned up when possible, and
the copy is skipped for that run.

Snapshot replacement is a metadata replace operation. Uploading
`.kitchensync/snapshot.db` must write the new database to a TMP path and close
it before changing the live snapshot path. Replacement must work on transports
whose rename does not overwrite an existing target. The implementation may
delete the old `.kitchensync/snapshot.db` immediately before renaming the staged
snapshot into place. If the final rename fails, the staged snapshot is left
under TMP for `--xd` cleanup and the failure is logged.

### File Copy

Each transfer is a `(src_peer, path, dst_peer, path)` pair. A transfer acquires one connection from the source peer's pool and one from the destination peer's pool before starting (see concurrency.md for pool semantics - SFTP pools are keyed by user+host+port, so two SFTP peers that share user+host+port share a pool; `file://` peers have no pool).

1. **Transfer** to TMP staging on destination: `<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>`
2. **If** the destination already has a file at the target path, **displace** it to `<file-parent>/.kitchensync/BAK/<timestamp>/<basename>`
3. **Swap** - rename from TMP to final path (same filesystem, atomic)
4. **Set mod_time** - set the destination file's modification time to the winning mod_time from the decision (not re-read from the source)
5. **Clean up** empty TMP directories

Content is streamed, not buffered entirely in memory. Each transfer uses a
generous bounded buffer sized for modern hardware: large enough to avoid
excessive syscall and transport round trips, but still bounded so many
concurrent transfers cannot consume unbounded memory. The default chunk size
must be at least 1 MiB unless a transport-specific constraint requires smaller
chunks. Each streamed transfer spawns two concurrent tasks connected by a
bounded channel: a reader task that reads chunks from the source and pushes them
into the channel, and a writer task that pulls chunks and writes them to the
destination. The reader and writer operate simultaneously - the channel provides
backpressure (reader blocks when the channel is full, writer blocks when it is
empty). A single-loop read-then-write pattern is not acceptable.

When both source and destination are local filesystems, KitchenSync may use the
host filesystem's native file-copy primitive to populate the TMP staging file
instead of the generic streaming pump. The same safety boundary still applies:
copy to TMP first, displace any existing destination to BAK, rename TMP into
place, set the winning mod_time, and clean up temporary staging on failure.
On transfer failure, delete the TMP staging file/directory for that transfer
before returning the connections to the pool.

### Displace to BAK

Each displacement is a `(peer, path)` pair executed inline during the combined-tree walk. Before performing the rename, create the destination directory (`<parent>/.kitchensync/BAK/<timestamp>/`) and any missing parents if it does not already exist. The entry at `path` is renamed to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. A displaced directory is moved as a single rename, preserving its entire subtree.

## Logging

All output produced by KitchenSync goes to stdout. stderr must remain empty across argument parsing, sync execution, and shutdown - including output from any third-party library pulled in transitively (SLF4J, java.util.logging, jsch, etc.). Configure transitive logging frameworks at startup so that their output is either suppressed entirely or routed to stdout. A user running `2>/dev/null` must never miss diagnostic information; a user running `2>&1` must never see duplicate lines.

Every file copy and every deletion (displacement to BAK/) is logged at `info` level with a short format:

- Copy: `C <relative-path>`
- Delete: `X <relative-path>`
- Directory status: `? <relative-directory>`

Logged once per decision, not per peer. This gives the user visible progress output.

Directory status is a quiet-period progress line. During the combined-tree walk, KitchenSync tracks the directory currently being listed or compared. If no stdout line has been written for `--dir-status` seconds, it logs `? <relative-directory>` at `info` level. The root directory is logged as `? .`. A value of `--dir-status 0` disables directory status logging.

Verbosity levels (`-vl`, ordered least-to-most verbose: `error` < `info` < `debug` < `trace`) are cumulative - each level emits everything the lower levels emit plus its own additions. The spec currently defines messages at three of the four levels: `error` (the error conditions enumerated in section Errors below, nonfatal diagnostics for skipped peers and recoverable operation failures, and listing errors described in multi-tree-sync.md section Algorithm), `info` (the `C`/`X` progress lines above and `?` directory status lines), and `trace` (pool acquire/release events, see concurrency.md section Trace Logging). No debug-specific messages are defined; `-vl debug` is observationally identical to `-vl info` until debug-only messages are specified.

Failed file-transfer diagnostics must identify the relative path, the
destination peer URL, the failed phase, and the transport error category when
available. The failed phase is one of: `read_source`, `write_tmp`,
`displace_existing`, `rename_final`, `set_mod_time`, or `cleanup`.

Example:

```text
transfer failed for kitchensync.exe to sftp://ace@host/path: displace_existing: permission_denied
```

## TMP Staging

Staged near the target for same-filesystem atomic rename. Inside `.kitchensync/` to stay hidden. UUID per transfer prevents collisions. The `<timestamp>` in the path uses the format defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Stale dirs cleaned after `--xd` days (default: 2).

## BAK Directory

Displaced entries are recoverable from BAK/ until cleaned. BAK/ is created at the parent directory of each displacement (co-located in `.kitchensync/` at every directory level), not aggregated at the sync root. The `<timestamp>` in the path uses the format defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Cleaned after `--bd` days (default: 90).

## Peer Transports

Each peer is reached via a transport. For `sftp://` URLs the transport is the `sftp-protocol` component (see `decomposition.md`); for `file://` URLs it is the host language's standard library, used directly. Both expose the same set of operations against the peer's filesystem; the kitchensync entry point dispatches to the right transport per peer based on the URL scheme.

### Required Operations

Every transport must support:

| Operation                      | Description                                                                     |
| ------------------------------ | ------------------------------------------------------------------------------- |
| `list_dir(path)`               | List immediate children (name, is_dir, mod_time, byte_size). byte_size is file size in bytes for files, or -1 for directories |
| `stat(path)`                   | Return mod_time, byte_size, is_dir; or "not found"                              |
| `open_read(path)` -> handle     | Open a file for streaming read                                                  |
| `read(handle, max_bytes)`      | Pull the next chunk; returns bytes or EOF                                       |
| `close_read(handle)`           | Close a read handle                                                             |
| `open_write(path)` -> handle    | Open a file for streaming write (creates the file and parent dirs as needed)    |
| `write(handle, bytes)`         | Push the next chunk                                                             |
| `close_write(handle)`          | Finalize the write (flush + close)                                              |
| `rename(src, dst)`             | Same-filesystem rename (for TMP -> final swap)                                  |
| `delete_file(path)`            | Remove a file                                                                   |
| `create_dir(path)`             | Create directory (and parents as needed)                                        |
| `delete_dir(path)`             | Remove empty directory                                                          |
| `set_mod_time(path, time)`     | Set file/directory modification time                                            |

`list_dir` returns only regular files and directories. Symbolic links, special files (devices, FIFOs, sockets), and any other non-regular entry types are silently omitted by the implementation. The same applies to `stat`: if the path is a symlink or special file, return "not found."

The streaming pipeline (two concurrent tasks connected by a bounded channel - see section"File Copy") is implemented above the transports, not inside either one. The pipeline's reader task loops `source.read(handle, ...)` -> channel; the writer task loops channel -> `dest.write(handle, ...)`. Each transport just provides the chunk-level primitives.

### Error Semantics

All operations return the same error categories regardless of transport: not found, permission denied, I/O error. Sync logic never matches on transport-specific errors. Network failures (connection drop, timeout) surface as I/O errors - sync logic doesn't distinguish "disk read failed" from "SFTP channel died."

### Testability

Each transport is independently testable via its own component-level interface (the `sftp-protocol` component exposes its surface for direct testing; the file-stdlib path is exercised by end-to-end tests with `file://` peers). The full sync is tested end-to-end via the CLI with mixed transports - typical end-to-end tests use `file://` peers under a temporary directory; additional tests exercise `sftp://` peers against localhost. See `TESTING-GUIDELINES.md`.

SFTP replacement behavior must be tested against a local SFTP fixture or fake
transport that rejects plain rename-over-existing while allowing ordinary
create, write, delete, and rename-to-new-path operations. KitchenSync must pass
that fixture for both snapshot replacement and user-file replacement. Tests
must not depend on a personal LAN host or external account.

## Errors

- **Argument errors** on non-help invocations (too few peers, multiple `+` peers, invalid settings) -> print to stdout, exit 1
- **No snapshots and no canon** -> print suggestion (`+`), exit 1
- **Unreachable peer** -> skip, log at error level, continue with others
- **Canon peer unreachable** -> exit 1
- **Fewer than two reachable peers** -> exit 1
- **No contributing peer reachable** (all reachable peers are subordinate after auto-subordination) -> print `No contributing peer reachable - cannot make sync decisions`, exit 1
- **Transfer failure** -> log, skip file (re-discovered next run)
- **Displacement failure** (cannot rename to BAK/) -> log error, skip the displacement (file remains in place). If the displacement was part of a file copy sequence, the copy is also skipped (TMP staging file is cleaned up)
- **TMP staging failure** (cannot create staging directory or write staging file) -> treat as transfer failure
- **`set_mod_time` failure** (after a completed copy - file is already in place) -> log at error level; the copy is not undone. The destination snapshot row already records the winning mod_time, so the discrepancy will be detected and corrected on the next run
- **Snapshot upload failure** -> log error, leave TMP staging file for `--xd` cleanup (peer's snapshot will be stale on next run, leading to redundant but correct copies)

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BAK/.
