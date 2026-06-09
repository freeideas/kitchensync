# Sync

KitchenSync is a native Rust command-line executable for Windows, Linux, and
macOS.

## Command Line

```
kitchensync [options] <peer> <peer> [<peer>...]
```

No arguments: print help and exit 0 (see help.md).

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
kitchensync +[sftp://192.168.1.50/photos,sftp://nas.vpn/photos] /local/photos
```

The `+`/`-` prefix goes on the bracket, not on individual URLs inside.

### Per-URL Settings

Query-string parameters on a URL override connection settings for that URL:

```
"sftp://host/path?timeout-conn=60&timeout-idle=10"
```

| Param          | Meaning             | Global flag      |
| -------------- | ------------------- | ---------------- |
| `timeout-conn` | Connection timeout  | `--timeout-conn` |
| `timeout-idle` | Idle keep-alive TTL | `--timeout-idle` |

Query-string parameters are stripped during URL normalization - they are not part of the URL's identity.

`max-copies` is not valid in a URL query string. `--max-copies` is a global
active-copy limit for the whole run.

### Command-Line Excludes

`-x <relative-path>` excludes one path from scanning, decisions, copying,
deletion, displacement, and snapshot updates. The flag is repeatable.

Exclude paths are slash-separated relative paths in the same format KitchenSync
prints in progress output:

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

Command-line excludes are in addition to built-in excludes. They cannot include
or override `.kitchensync/`, `.git/`, symbolic links, or special files.

### Global Options

| Flag              | Default | Meaning                                                                     |
| ----------------- | ------- | --------------------------------------------------------------------------- |
| `--dry-run`       | off     | Read and plan as realistically as possible, but make no peer changes        |
| `--max-copies`    | 10      | Max concurrent copies across the whole run                                  |
| `--retries-copy`  | 3       | Give up copying after this many tries                                      |
| `--retries-list`  | 3       | Give up listing after this many tries                                      |
| `--timeout-conn`  | 30      | Seconds for SSH handshake timeout                                           |
| `--timeout-idle`  | 30      | SFTP idle keep-alive TTL (seconds)                                          |
| `--verbosity`     | `info`  | Verbosity level (error, info, debug, trace)                                 |
| `-x`              | -       | Exclude a relative path from scanning and copying; repeatable               |
| `--keep-tmp-days` | 2       | Delete stale TMP staging after N days                                       |
| `--keep-bak-days` | 90      | Delete displaced files (BAK/) after N days                                  |
| `--keep-del-days` | 180     | Forget deletion records after N days                                        |

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

Each listed credential source is a required part of the fallback chain, not an
example. If one source is absent or rejected, KitchenSync must continue to the
next source in this exact order. In particular, a host that accepts only
`~/.ssh/id_ed25519` and does not accept `~/.ssh/id_rsa` must be reachable
without an inline password or SSH agent.

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

1. Parse command line. A help invocation is no arguments at all; it prints the help text to stdout and exits 0 (see `help.md`). For non-help invocations, validate: at least two peers, at most one `+` peer, no unrecognized flags, and all option values are valid (e.g., `--max-copies`, `--timeout-conn`, `--timeout-idle`, `--keep-tmp-days`, `--keep-bak-days`, `--keep-del-days`, `--retries-copy`, and `--retries-list` are positive integers; `--verbosity` is one of `error`/`info`/`debug`/`trace`; every `-x` path is a valid relative slash path; URL query parameters are limited to `timeout-conn` and `timeout-idle`). On any validation error, print the error message followed by the help text and exit 1.
2. Connect to all peers in parallel. In normal runs, auto-create the peer's root directory (and any missing parents) if it does not exist - for both `file://` and `sftp://` URLs. In `--dry-run`, do not create missing peer roots or parents; a URL whose root path does not already exist is treated as unreachable for that run. For peers with fallback URLs (bracket syntax), try URLs in order; first that connects wins. Skip unreachable peers with an error-level diagnostic. If directory creation fails in a normal run, treat the peer as unreachable (try next fallback URL).
3. If fewer than two peers are reachable, exit with error.
4. If canon peer (`+`) is unreachable, exit with error.
5. In normal runs, recover any incomplete `.kitchensync/SWAP/snapshot.db/` state, then download each peer's `.kitchensync/snapshot.db` to a local temp directory (`{tmp}/{uuid}/snapshot.db`). In `--dry-run`, skip peer-side snapshot SWAP recovery and download the live `.kitchensync/snapshot.db` as-is. If a peer has no `snapshot.db` (transport returns 'not found'), create a new empty one locally. If recovery or download fails with any other error (I/O error, permission denied), treat the peer as unreachable: log an error-level diagnostic and exclude it from the reachable set, then re-evaluate steps 3-4 against the updated set and exit with the corresponding error if either check now fails.
6. Peers whose `.kitchensync/snapshot.db` did not exist on disk (i.e., a new empty database was created in step 5) are automatically treated as subordinate unless they are the canon peer (`+`). If no peer has any snapshot data and no canon peer (`+`) is designated, print `First sync? Mark the authoritative peer with a leading +` and exit 1.
7. If no contributing (non-subordinate) peer is reachable after auto-subordination, exit with error: `No contributing peer reachable - cannot make sync decisions`

## Run

1. Run combined-tree walk (see multi-tree-sync.md)
   - Directory creation and displacement (to BAK/) inline
   - File copies enqueued for concurrent execution
   - Snapshot updated during traversal
   - Global active-copy limit enforced (see concurrency.md)
2. Opportunistically purge old snapshot rows while traversal is already running, or after copying has already started. This maintenance must not delay the first directory scan or the first eligible copy.
3. Wait for all enqueued file copies to complete
4. In normal runs, write updated snapshots back to peers using SWAP staging
   (see database.md). Failed uploads leave SWAP state that is recovered on the
   next run. In `--dry-run`, skip this step; updated local temp snapshots are
   not uploaded back to peers.
5. Disconnect all peers
6. Log completion, exit 0

## Operation Queue

File copies are enqueued during the combined-tree walk and executed concurrently, subject to the global active-copy limit (see concurrency.md). There is no loading phase that scans the whole tree before copy work begins. As soon as the first scanned directory produces copy work, copy workers may begin reading and copying those files while traversal continues into later directories.

Each queued copy carries its own try count. `--retries-copy` is the maximum number of total tries for that copy, including the first try. Directory creation and displacement to BAK/ run inline during the walk - both are same-filesystem operations that subsequent steps may depend on.

### Dry Run

`--dry-run` makes the run as realistic as possible without changing any peer.
KitchenSync still connects to peers, downloads snapshots to local temp files,
lists directories, reads source files for queued copies, updates the local temp
snapshot databases, exercises the copy queue, applies try-limit behavior, and
emits the same `C`/`X` progress lines.

Dry-run copy work acquires copy slots and reads source files, but destination
write, swap, archive, delete, and mod_time operations are planned only and are
not executed against peers.

In dry-run mode, KitchenSync must not create, modify, rename, delete, displace,
or upload anything through a `file://` or `sftp://` peer URL. This means:

- no peer directories are created;
- missing peer root directories or parents are treated as unreachable, not
  created;
- no TMP, SWAP, or BAK directories are created on peers;
- no destination files are written;
- no destination files are displaced or deleted;
- no modification times are set on peers;
- updated local temp snapshots are not uploaded back to peers;
- BAK/TMP cleanup and SWAP recovery on peers are skipped.

Dry-run output includes the phrase `dry run` at least once on stdout. The local
temp databases may be written because they are local working state, not peer
state.

### Rename Compatibility

KitchenSync must not assume that transport `rename(src, dst)` overwrites an
existing destination. This matters for SFTP servers that reject rename when
`dst` already exists.

User data replacement is a recoverable swap. When a copied file would replace
an existing file, KitchenSync must first write the replacement to the peer's
SWAP `new` path, then move the existing file to the peer's SWAP `old` path,
then move `new` into the final path. After the final path exists, `old` is moved
to BAK. If the run stops after the existing file is moved, SWAP `old` is durable
proof that the missing final path is an incomplete KitchenSync swap, not a user
deletion.

If moving the existing destination to SWAP `old` fails, the original
destination must remain in place, staged files must be cleaned up when possible,
and the copy is skipped for that run.

Snapshot replacement is a metadata replace operation. Uploading
`.kitchensync/snapshot.db` uses the same SWAP rule as user files: write the new
database to SWAP `new`, move the old snapshot to SWAP `old`, move `new` into
place, then remove `old`. If the run stops during the swap, startup recovery
must repair or complete the snapshot swap before deciding whether the peer has
snapshot history.

### File Copy

Each transfer is a `(src_peer, path, dst_peer, path)` pair. A transfer acquires one global copy slot before starting. At most `--max-copies` transfers may hold copy slots at the same time across the whole run, regardless of peer scheme.

1. **Transfer** to SWAP `new`: `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new`
2. **If** the destination already has a file at the target path, rename it to SWAP `old`: `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old`
3. **Swap in** - rename SWAP `new` to the final path
4. **Set mod_time** - set the destination file's modification time to the winning mod_time from the decision (not re-read from the source)
5. **Archive old** - if SWAP `old` exists, rename it to `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>`
6. **Clean up** empty SWAP directories

Content is streamed with bounded buffering. Each active transfer uses one or
more fixed-size buffers whose total size is independent of the file size.
KitchenSync must not require the entire file to be buffered in memory before
writing begins.

When both source and destination are local filesystems, KitchenSync may use the
host filesystem's native file-copy primitive to populate the SWAP `new` file
instead of the generic streaming pump. The same safety boundary still applies:
copy to SWAP `new` first, move any existing destination to SWAP `old`, rename
`new` into place, set the winning mod_time, move `old` to BAK, and clean up
temporary staging on failure. On transfer failure before the existing
destination is moved to SWAP `old`, delete the SWAP `new` file/directory for
that transfer before releasing the copy slot. If the queued copy has not yet reached its
`--retries-copy` total-try limit, move it to the back of the queue. Otherwise
mark it failed for this run.

### Displace to BAK

Each displacement is a `(peer, path)` pair executed inline during the combined-tree walk. Before performing the rename, create the destination directory (`<parent>/.kitchensync/BAK/<timestamp>/`) and any missing parents if it does not already exist. The entry at `path` is renamed to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. A displaced directory is moved as a single rename, preserving its entire subtree.

## Logging

All output produced by KitchenSync goes to stdout. stderr must remain empty across argument parsing, sync execution, and shutdown. A user running `2>/dev/null` must never miss diagnostic information; a user running `2>&1` must never see duplicate lines.

Progress is the per-action `C`/`X` line output described in `concurrency.md`,
emitted to stdout in the order the actions happen. The same lines are produced
whether or not stdout is a terminal.

Verbosity levels (`--verbosity`, ordered least-to-most verbose: `error` < `info` < `debug` < `trace`) are cumulative - each level emits everything the lower levels emit plus its own additions. The spec currently defines messages at three of the four levels: `error` (the error conditions enumerated in section Errors below, nonfatal diagnostics for skipped peers and recoverable operation failures, and listing errors described in multi-tree-sync.md section Algorithm), `info` (the `C`/`X` progress lines, see concurrency.md section Progress Output), and `trace` (copy-slot acquire/release events, see concurrency.md section Trace Logging). No debug-specific messages are defined; `--verbosity debug` is observationally identical to `--verbosity info` until debug-only messages are specified.

Failed file-transfer diagnostics must identify the relative path, the
destination peer URL, the failed phase, and the transport error category when
available. The failed phase is one of: `read_source`, `write_swap_new`,
`move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or
`cleanup`.

Example:

```text
transfer failed for kitchensync.exe to sftp://ace@host/path: move_existing_to_swap_old: permission_denied
```

## TMP Staging

TMP staging is used for temporary metadata and cleanup work that does not
replace a live user path. It lives inside `.kitchensync/` to stay hidden. UUID
per transfer prevents collisions. The `<timestamp>` in the path uses the format
defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Stale dirs cleaned after
`--keep-tmp-days` days (default: 2).

## SWAP Directory

SWAP staging is used for replacing an existing file without losing evidence of
an interrupted swap. For a target `<parent>/<basename>`, the SWAP paths are:

- `<parent>/.kitchensync/SWAP/<encoded-basename>/new`
- `<parent>/.kitchensync/SWAP/<encoded-basename>/old`

`<encoded-basename>` is the basename percent-encoded when needed so it can be
used as one path segment on every supported transport. Before starting a
replacement for a path, KitchenSync must recover or fail any existing SWAP
directory for that basename.

Snapshot replacement uses `.kitchensync/SWAP/snapshot.db/new` and
`.kitchensync/SWAP/snapshot.db/old`.

## BAK Directory

Displaced entries are recoverable from BAK/ until cleaned. BAK/ is created at the parent directory of each displacement (co-located in `.kitchensync/` at every directory level), not aggregated at the sync root. The `<timestamp>` in the path uses the format defined in database.md (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). Cleaned after `--keep-bak-days` days (default: 90).

## Peer Transports

Each peer is reached through filesystem operations selected by URL scheme. `sftp://` URLs use SSH/SFTP. `file://` URLs and bare paths use local filesystem operations. Both schemes must provide the same behavior to the sync engine.

### Required Operations

Every transport must support:

- `list_dir(path)`:
  List immediate children: name, `is_dir`, `mod_time`, and `byte_size`.
  `byte_size` is the file size in bytes for files, or -1 for directories.
- `stat(path)`:
  Return `mod_time`, `byte_size`, and `is_dir`; or "not found".
- `open_read(path)` -> handle:
  Open a file for streaming read.
- `read(handle, max_bytes)`:
  Pull the next chunk; returns bytes or EOF.
- `close_read(handle)`:
  Close a read handle.
- `open_write(path)` -> handle:
  Open a file for streaming write, creating the file and parent directories as
  needed.
- `write(handle, bytes)`:
  Push the next chunk.
- `close_write(handle)`:
  Finalize the write: flush and close.
- `rename(src, dst)`:
  Same-filesystem rename. The destination must not already exist.
- `delete_file(path)`:
  Remove a file.
- `create_dir(path)`:
  Create a directory and any needed parents.
- `delete_dir(path)`:
  Remove an empty directory.
- `set_mod_time(path, time)`:
  Set file/directory modification time.

`list_dir` returns only regular files and directories. Symbolic links, special files (devices, FIFOs, sockets), and any other non-regular entry types are silently omitted by the implementation. The same applies to `stat`: if the path is a symlink or special file, return "not found."

Streaming and bounded buffering are implemented above scheme-specific
filesystem operations. The scheme-specific layer provides the chunk-level read
and write primitives.

### Error Semantics

All operations return the same error categories regardless of transport: not found, permission denied, I/O error. Sync logic never matches on transport-specific errors. Network failures (connection drop, timeout) surface as I/O errors - sync logic doesn't distinguish "disk read failed" from "SFTP channel died."

### Testability

The full sync is tested end-to-end via the CLI with mixed peer schemes. Typical end-to-end tests use local peers under a temporary directory; additional tests exercise `sftp://` peers against localhost. See `TESTING-GUIDELINES.md`.

SFTP replacement behavior must be tested against a local SFTP fixture or fake
transport that rejects plain rename-over-existing while allowing ordinary
create, write, delete, and rename-to-new-path operations. KitchenSync must pass
that fixture for both snapshot replacement and user-file replacement by using
SWAP recovery, not rename-over-existing. Tests must not depend on a personal LAN
host or external account.

## Errors

- **Argument errors** on non-help invocations (too few peers, multiple `+` peers, invalid settings) -> print to stdout, exit 1
- **No snapshots and no canon** -> print suggestion (`+`), exit 1
- **Unreachable peer** -> skip, log at error level, continue with others
- **Directory listing failure** -> try that listing up to `--retries-list` total times; if it still fails, exclude that peer for that directory subtree without modifying its snapshot rows or peer files under that subtree. If the failed peer is the canon peer (`+`), skip decisions for that directory subtree for all peers
- **Canon peer unreachable** -> exit 1
- **Fewer than two reachable peers** -> exit 1
- **No contributing peer reachable** (all reachable peers are subordinate after auto-subordination) -> print `No contributing peer reachable - cannot make sync decisions`, exit 1
- **Transfer failure before SWAP `old` exists** -> clean up staging and requeue the copy later if its total try count is below `--retries-copy`; otherwise log final failure and skip the file for this run (re-discovered next run)
- **Transfer failure after SWAP `old` exists** -> leave SWAP state in place, log error, and recover it before making decisions for that directory again
- **Archive old failure** (cannot rename SWAP `old` to BAK after the replacement is in place) -> log error and leave SWAP `old` for later recovery
- **Displacement failure** (cannot rename to BAK/) -> log error and skip the displacement (file remains in place)
- **TMP or SWAP staging failure** (cannot create staging directory or write staging file) -> treat as transfer failure
- **`set_mod_time` failure** (after a completed copy - file is already in place) -> log at error level; the copy is not undone. The destination snapshot row already records the winning mod_time, so the discrepancy will be detected and corrected on the next run
- **Snapshot upload failure before SWAP `old` exists** -> log error and keep the old live snapshot; any SWAP `new` is handled by startup recovery
- **Snapshot upload failure after SWAP `old` exists** -> log error and leave SWAP state in place for startup recovery

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BAK/.
