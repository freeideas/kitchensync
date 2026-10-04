# Sync

KitchenSync is a native Rust command-line executable for Windows, Linux, and macOS.

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
- **(none)** - normal bidirectional peer. Contributes and receives based on its history (see state.md).

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

Query-string parameters are stripped from a URL once they have been read: they are settings, not part of the location. Bare local paths are made absolute. The stripped, absolute form is what KitchenSync shows in its output.

`parallel` is not valid in a URL query string. `--parallel` is a global active-copy limit for the whole run.

### Excludes

Excludes decide which entries KitchenSync pretends do not exist for a run. They use the same pattern language as `.gitignore`, so anyone who has written one already knows the rules:

- A pattern is matched against each entry's slash-separated path relative to the sync root, the same form KitchenSync prints in progress output.
- `*` matches any run of characters except `/`; `?` matches one such character; `[abc]` and `[a-z]` match one character from a set; `**` matches any number of path segments (`a/**/b`, `**/name`, `dir/**`).
- A pattern with no `/` (other than a trailing one) matches an entry with that name at any depth: `.DS_Store`, `*.tmp`.
- A pattern containing a `/` is anchored at the sync root: `movx/quarantined` matches only that path. A leading `/` is allowed and means the same thing.
- A trailing `/` makes the pattern match directories only: `build/`.
- Excluding a directory excludes everything beneath it.
- A `#` at the start of a line is a comment; blank lines are ignored; `\#` and `\!` escape a leading `#` or `!`.
- A leading `!` negates: the entry is included again even if an earlier pattern excluded it. The last matching pattern wins.

Sources of patterns, applied in this order so that later ones can override earlier ones:

1. Built-in patterns for operating-system litter: `.DS_Store`, `._*`, `Thumbs.db`, `desktop.ini`. Turn one back on with a negation, for example `-x '!.DS_Store'`.
2. `<root>/.kitchensync/ignore` on every reachable peer, in command-line peer order; each file is read at startup (in dry runs too) and applies to the whole run on every peer.
3. `-x <pattern>` from the command line, repeatable, in the order given. `-x @<file>` reads patterns, one per line, from a local file instead.

An excluded entry is skipped in scanning, decisions, copying, deletion, displacement, and state updates. Existing excluded files or directories on any peer are left untouched, and existing state lines for excluded paths are not consulted during the run; they are carried forward unchanged when the state is written.

Nothing can include `.kitchensync/`, `.git/`, symbolic links, or special files; those are excluded before patterns are considered.

An `-x` value that is empty, contains a NUL character, or names an `@` file that cannot be read is an argument error.

### Global Options

| Flag              | Default | Meaning                                                                     |
| ----------------- | ------- | --------------------------------------------------------------------------- |
| `--dry-run`       | off     | Read and plan as realistically as possible, but make no peer changes        |
| `--parallel`      | 5       | Files copied at the same time across the whole run                          |
| `--retries-copy`  | 3       | Give up copying after this many tries                                      |
| `--retries-list`  | 3       | Give up listing after this many tries                                      |
| `--timeout-conn`  | 30      | Seconds for SSH handshake timeout                                           |
| `--timeout-idle`  | 30      | SFTP idle keep-alive TTL (seconds)                                          |
| `--verbosity`     | `info`  | Verbosity level (error, info, debug, trace)                                 |
| `--rollback`      | -       | Roll the given peers back to a timestamp, then exit (see Rollback)          |
| `--undo`          | -       | Roll the given peers back to just before their newest run                   |
| `-x`              | -       | Exclude by gitignore-style pattern, or `@file` of patterns; repeatable       |
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

Each listed credential source is a required part of the fallback chain, not an example. If one source is absent or rejected, KitchenSync must continue to the next source in this exact order. In particular, a host that accepts only `~/.ssh/id_ed25519` and does not accept `~/.ssh/id_rsa` must be reachable without an inline password or SSH agent.

Host keys verified via `~/.ssh/known_hosts`. Unknown hosts rejected.

## Canon Peer (`+`)

A canon peer is authoritative - its state wins all conflicts unconditionally.

Canon is never required. History is kept per peer in its state (see state.md), and a directory for which no contributing peer has history is merged additively: every live entry is New, the newest mod_time wins for files, directories exist everywhere, and nothing in that directory is deleted or displaced except for type conflicts and bringing subordinate peers into conformance.

Most people expect a first sync to copy one way, the way rsync does, so KitchenSync says plainly when it is doing something else. If the sync root has no history (see state.md) on any reachable contributing peer and no `+` peer was given, KitchenSync prints exactly this one stdout line before any progress output, at every verbosity level, and continues:

```text
first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative
```

Mark a peer with `+` when you want that peer's contents to win instead.

## Subordinate Peer (`-`)

A subordinate peer does not contribute to decisions. During the decision phase, its files are invisible - decisions are made using only normal and canon peers. After decisions are made, the subordinate peer is made to match the outcome: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it.

A peer is automatically treated as subordinate for the run when that peer has no history at the sync root while at least one other reachable peer does, unless it is the canon peer (`+`). The `-` prefix is redundant for such a peer but harmless. This means a peer joining an established group always receives the group's state without influencing decisions.

If no reachable peer has history at the sync root, nobody is auto-subordinated: that is the first-sync merge described under "Canon Peer".

A subordinate peer's state is still read and written. On future runs (without `-`), the peer participates normally using that history.

## Startup

1. Parse command line. A help invocation is no arguments at all; it prints the help text to stdout and exits 0 (see `help.md`). For non-help invocations, validate: at least two peers, at most one `+` peer, no unrecognized flags, and all option values are valid (e.g., `--parallel`, `--timeout-conn`, `--timeout-idle`, `--keep-bak-days`, `--keep-del-days`, `--retries-copy`, and `--retries-list` are positive integers; `--verbosity` is one of `error`/`info`/`debug`/`trace`; every `-x` value is a valid pattern or a readable `@file`; URL query parameters are limited to `timeout-conn` and `timeout-idle`; `--rollback` carries a timestamp in the standard format and is not combined with `--undo`). A `--rollback` or `--undo` invocation is validated differently: one or more peers, no `+` or `-` prefixes (see Rollback). On any validation error, print the error message followed by the help text and exit 1.
2. Connect to all peers in parallel. In normal runs, auto-create the peer's root directory (and any missing parents) if it does not exist - for both `file://` and `sftp://` URLs. In `--dry-run`, do not create missing peer roots or parents; a URL whose root path does not already exist is treated as unreachable for that run. For peers with fallback URLs (bracket syntax), try URLs in order; first that connects wins. A reachable peer carries the connected peer root handle selected at startup: a local root handle for `file://`, or the established SSH/SFTP session plus remote root path for `sftp://`. Skip unreachable peers with an error-level diagnostic. If directory creation fails in a normal run, treat the peer as unreachable (try next fallback URL).
3. If fewer than two peers are reachable, exit with error.
4. If canon peer (`+`) is unreachable, exit with error.
5. Nothing is downloaded at startup. For each reachable peer, check only whether the sync root has history: does `.kitchensync/state.txt` exist there (or, failing that, `.kitchensync/manifest.txt`; see state.md, "Per-directory manifests")? In normal runs, repair an interrupted replacement of `state.txt` and `runs.txt` first (see state.md, "Writing"), so a run that was stopped mid-rewrite is not mistaken for a peer with no history. In `--dry-run`, repair nothing and treat the root as having history if any of `state.txt`, `state.txt.new`, `state.txt.old`, or `manifest.txt` is present. Then read the state into memory. In normal runs, delete expired `BAK/` folders and journals (see state.md, "BAK"). If the check fails with anything other than 'not found' (I/O error, permission denied), treat the peer as unreachable: log an error-level diagnostic and exclude it from the reachable set, then re-evaluate steps 3-4 against the updated set and exit with the corresponding error if either check now fails.
6. A reachable peer with no history is automatically treated as subordinate for the run, unless it is the canon peer (`+`), and unless no reachable peer has history at all. If no reachable contributing peer has history and no canon peer (`+`) is designated, nobody is auto-subordinated; print this line to stdout, exactly once, before any progress output and at every verbosity level (after the `dry run` line in a dry run), then continue the run: `first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative`
7. If no contributing (non-subordinate) peer is reachable - for example every reachable peer was marked `-` on the command line and there is no canon peer - exit with error: `No contributing peer reachable - cannot make sync decisions`

## Run

1. At `info` verbosity and above, print the rollback hint line (see Logging) as the first progress line of the run
2. Run combined-tree walk (see multi-tree-sync.md)
   - Directory creation and displacement (to BAK/) inline
   - File copies enqueued for concurrent execution
   - Each directory's state lines settled on a peer once that directory's entries are decided and its copies into that peer have finished or failed (see state.md, "Writing")
   - Global active-copy limit enforced (see concurrency.md)
3. Wait for all enqueued file copies to complete
4. Write each peer's state (see state.md, "Writing"; never in `--dry-run`), close its journal, and disconnect
5. Print the completion line and exit. If every copy succeeded and every displacement and state write succeeded, print exactly `sync complete` as one stdout line and exit 0. Otherwise print exactly `sync complete with N failures`, where N counts the copies given up on after `--retries-copy` tries plus the failed displacements and failed state writes, and exit 2.

## Operation Queue

File copies are enqueued during the combined-tree walk and executed concurrently, subject to the global active-copy limit (see concurrency.md). There is no loading phase that scans the whole tree before copy work begins. As soon as the first scanned directory produces copy work, copy workers may begin reading and copying those files while traversal continues into later directories. No copy waits for the walk to finish.

Each queued copy carries its own try count. `--retries-copy` is the maximum number of total tries for that copy, including the first try. Directory creation and displacement to BAK/ run inline during the walk - both are same-filesystem operations that subsequent steps may depend on.

### Rename Compatibility

KitchenSync must not assume that transport `rename(src, dst)` overwrites an existing destination. This matters for SFTP servers that reject rename when `dst` already exists.

User data replacement is a recoverable swap. When a copied file would replace an existing file, KitchenSync must first write the replacement to the peer's SWAP `new` path, then move the existing file to the peer's SWAP `old` path, then move `new` into the final path. After the final path exists, `old` is moved to BAK. If the run stops after the existing file is moved, SWAP `old` is durable proof that the missing final path is an incomplete KitchenSync swap, not a user deletion.

If moving the existing destination to SWAP `old` fails, the original destination must remain in place, staged files must be cleaned up when possible, and the copy is skipped for that run.

`state.txt` and `runs.txt` follow the same no-rename-over-existing rule, using their own `.new`/`.old` names inside `.kitchensync/` rather than SWAP; state.md ("Writing") defines the exact sequence and how an interrupted replacement is repaired.

### File Copy

Each transfer is a `(src_peer, path, dst_peer, path)` pair. A transfer acquires one global copy slot before starting. At most `--parallel` transfers may hold copy slots at the same time across the whole run, regardless of peer scheme.

1. **Transfer** to SWAP `new`: `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new`
2. **If** the destination already has a file at the target path, rename it to SWAP `old`: `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old`
3. **Swap in** - rename SWAP `new` to the final path
4. **Set mod_time** - set the destination file's modification time to the winning mod_time from the decision (not re-read from the source)
5. **Archive old** - if SWAP `old` exists, move it to BAK as a displacement of the target path (see "Displace to BAK")
6. **Clean up** empty SWAP directories (see "SWAP Directory" for what counts as empty)

When the copy completes, the journal gets the `X` line for old content that step 5 moved to BAK, if any, and then a `C` line for the target (see state.md, "Journal"); a rollback, undoing newest first, takes the new content away before it puts the old back. A copy that is tried again after a failure first recovers any SWAP directory its earlier try left for that target (see "SWAP Directory").

Content is streamed with bounded buffering. Each active transfer uses one or more fixed-size buffers whose total size is independent of the file size. KitchenSync must not require the entire file to be buffered in memory before writing begins.

When both source and destination are local filesystems, KitchenSync may use the host filesystem's native file-copy primitive to populate the SWAP `new` file instead of the generic streaming pump. The same safety boundary still applies: copy to SWAP `new` first, move any existing destination to SWAP `old`, rename `new` into place, set the winning mod_time, move `old` to BAK, and clean up temporary staging on failure. On transfer failure before the existing destination is moved to SWAP `old`, delete the SWAP `new` file/directory for that transfer before releasing the copy slot. If the queued copy has not yet reached its `--retries-copy` total-try limit, move it to the back of the queue. Otherwise mark it failed for this run.

### Moved Files

Moving or renaming a file on one peer looks to the walk like a deletion at the old path plus a new file at the new path. Every other peer would then displace its copy at the old path to BAK and receive the file at the new path again: for a reorganized media library, hundreds of gigabytes the receiving peer already holds. Instead, a copy of a file of at least 1 MiB (1,048,576 bytes) first looks for that file on the destination peer under another name, and moves it into place when it finds it. Smaller files are always transferred: sending them costs little, and a match is less telling.

**Candidates.** Each peer has an index of files it holds or recently held, built at startup without reading anything beyond what startup already reads:

- every live file line of 1 MiB or more in the peer's state (see state.md);
- every file in the peer's BAK named by an `X` or `B` line of its journals.

During the run the index follows the peer: a displaced file or directory keeps its candidates, at their new BAK paths (the files inside a displaced directory are known from the state); a file displaced that the state did not list joins with the size and mod_time from its listing; a candidate moved into place leaves the index.

**Match.** A candidate is used for a copy of `path` to peer R when all of these hold:

1. Its byte size equals the decision's byte size, its modification time is within the 5-second tolerance of the decision's mod_time, and its path is not `path`. The name does not matter: renames are the common case.
2. If it is a live path (not under `BAK/`): the path is not excluded, the source peer's state has a line for it (the source once had it), and it does not exist on any contributing peer other than R. So the group has removed it everywhere else, and the walk would displace R's copy anyway.
3. A `stat` on R shows a file with that byte size and a modification time within the tolerance.
4. 64 KiB at the start of the file, 64 KiB in the middle (at offset `(size - 64 KiB) / 2`, rounded down), and the last 64 KiB read from the source and from the candidate are byte-for-byte equal. Any read error rules the candidate out.

When the walk queues the copy, it reserves the first unreserved candidate that meets rule 1 and, for a live path, the in-memory part of rule 2 (not excluded, and the source's state has a line for it). The rest of the check runs when the copy takes its copy slot, so copies never wait for the walk. If the reserved candidate fails, the copy tries the unreserved candidates in BAK that meet rule 1, then falls back to a transfer. Each candidate is used at most once.

**Move.** Step 1 of "File Copy" is skipped. Any existing destination file is moved to SWAP `old` (step 2), then the candidate is renamed from where it is straight to the final path (step 3), and steps 4 to 6 follow. The journal gets an `M` line naming where the file came from. The moved file never passes through SWAP `new`, so an interrupted run cannot mistake it for a cut-short transfer and delete it. If the final rename fails, SWAP `old` is moved back to the target path and the copy is retried as a normal transfer.

A move and a displacement on the same peer never run at the same time, and a move always uses the candidate's current location. When the walk decides to displace a file that is reserved by a copy still to come, it leaves the file for that copy and records it as displaced; it prints nothing for it. If the copy then does not use it, the copy displaces it then, printing the `X` line. When the walk reaches a path whose file was already moved away, there is nothing left to displace there, and that is not a failure. A directory is always displaced when the walk decides so; reserved files inside it are then moved from BAK.

**Output.** A copy of a file of 1 MiB or more prints its line when it takes its copy slot: `M<dst> <path>` for a move, `<src>C<dst> <path>` for a transfer (see concurrency.md, "Progress Output").

**Dry runs** check rules 1 to 3 in the walk, without reading file content, and print `M<dst> <path>` for a copy that would likely become a move. The candidate's old path then prints no `X` line, as in a real run.

### Displace to BAK

Each displacement is a `(peer, path)` pair executed inline during the combined-tree walk. The entry at `path` is renamed to `<root>/.kitchensync/BAK/<run>/<path>`, creating that path's parent directories first (see state.md, "BAK"). A displaced directory is moved as a single rename, preserving its entire subtree. The journal gets an `X` line, plus a `B` line for each file of 1 MiB or more that the state lists inside a displaced directory.

## Dry Run

`--dry-run` shows what the run would do without touching any peer. KitchenSync connects to peers, reads their state, lists directories, makes the same decisions, and emits the same `C`/`X`/`M` progress lines when progress output is enabled by verbosity.

Nothing else happens. File contents are not read, the copy queue is not exercised, no copy slot is taken, and no state or journal is written. A `C` line in a dry run means "this file would be copied", an `M` line "this file would likely be moved into place from elsewhere on that peer" (see "Moved Files"), and an `X` line "this path would be displaced".

In dry-run mode, KitchenSync must not create, modify, rename, delete, or displace anything through a `file://` or `sftp://` peer URL. This means:

- no peer directories are created;
- missing peer root directories or parents are treated as unreachable, not created;
- no SWAP or BAK directories are created on peers;
- no destination files are written;
- no destination files are displaced or deleted;
- no modification times are set on peers;
- no state, journal, or run log is written, replaced, or repaired;
- BAK cleanup, SWAP recovery, and conversion of per-directory manifests on peers are skipped.

At the start of every dry-run sync, before progress or completion output, KitchenSync prints exactly `dry run` as one stdout line. Dry-run progress output follows the same verbosity rules as normal progress output.

## Rollback

`--rollback <timestamp>` and `--undo` run instead of a sync. They put the given peers back the way they were at a moment in the past, using each peer's journals and `BAK/` (see state.md, "Rollback").

```
kitchensync --rollback 2024-03-05_08-00-01_120394Z c:/photos
kitchensync --undo c:/photos sftp://user@host/photos
```

- One or more peers. A single peer is allowed: rollback is done per peer, and peers are never compared with each other.
- A `+` or `-` prefix on a peer is an argument error.
- Fallback URL brackets and per-URL settings work as they do in a sync.
- `--rollback` needs a timestamp in the standard format (`YYYY-MM-DD_HH-mm-ss_ffffffZ`); anything else is an argument error.
- `--rollback` and `--undo` cannot be used together.

For each reachable peer, KitchenSync applies the rollback procedure in state.md ("Rollback") for the target time.

`--undo` picks the target time per peer from the run log: the start timestamp of the newest run recorded in `<root>/.kitchensync/runs.txt` on that peer (see "Run Log" below). A run's changes are spread over an interval, so the start timestamp is the only safe target: rolling back to it takes back everything the run did and nothing before. If the peer has no run log, KitchenSync falls back to one microsecond before the name of its newest journal; if there is none, print `nothing to undo for <peer>` (the peer URL as KitchenSync displays it) and treat that peer as done.

### Run Log

Every normal (non-dry-run) sync appends one line to `<root>/.kitchensync/runs.txt` on each reachable peer, right after printing the rollback hint and before any peer changes: the run's start timestamp, a tab, and the peers as shown in the hint. The file is replaced through the same write-new, move-old, rename-in sequence as `state.txt` and is trimmed to its last 1000 lines. It is per sync root: syncing `/X/b/c` and later `/X/b` produces a log in each. Rollback and dry runs do not write to it.

Progress output follows the usual verbosity rules (suppressed at `error`):

- `R<peer> <relpath>` for each entry put back where it was;
- `X<peer> <relpath>` for each entry that was added after the target time and is therefore removed. Removals are displaced to `BAK/` like any other deletion, so a rollback can itself be rolled back.

`--dry-run` combines with both options: the same `R`/`X` lines are printed and nothing on any peer is changed.

On completion, print exactly `rollback complete` as one stdout line and exit 0. If anything failed, print exactly `rollback complete with N failures` - N being the entries that could not be restored or removed plus the state writes that failed - and exit 2.

## Logging

All output produced by KitchenSync goes to stdout. stderr must remain empty across argument parsing, sync execution, and shutdown. A user running `2>/dev/null` must never miss diagnostic information; a user running `2>&1` must never see duplicate lines.

Progress is the per-action `C`/`M`/`X`/`R`/`S` line output described in `concurrency.md`. When progress lines are enabled by verbosity, they are emitted to stdout in the order the actions happen. The same lines are produced whether or not stdout is a terminal.

At `info` verbosity and above, the first progress line of every sync run is a rollback hint, so that the command needed to take the run back is on screen before anything changes:

```text
undo later with: kitchensync --rollback <timestamp> <peer> <peer>...
```

`<timestamp>` is generated at that moment - the start of the run - in the standard format. The peers are the reachable peers' URLs as KitchenSync displays them, in command-line order, without any `+` or `-` prefix, separated by single spaces and wrapped in double quotes if a URL contains a space. The line is printed once, right after startup succeeds: after the first-sync notice when it appears, and before any `C` or `X` line. At `error` verbosity it is suppressed like every other progress line. A dry run never prints it, because a dry run changes nothing and so there is nothing to undo.

Verbosity levels (`--verbosity`, ordered least-to-most verbose: `error` < `info` < `debug` < `trace`) are cumulative - each level emits everything the lower levels emit plus its own additions. Messages are defined at three of the four levels: `error` (the error conditions enumerated in section Errors below, nonfatal diagnostics for skipped peers and recoverable operation failures, and listing errors described in multi-tree-sync.md section Algorithm), `info` (the rollback hint and the `C`/`M`/`X`/`R`/`S` progress lines, see concurrency.md section Progress Output), and `trace` (copy-slot acquire/release events, see concurrency.md section Trace Logging). No message is specific to `debug`, so `--verbosity debug` prints exactly what `--verbosity info` prints.

Every run that reaches the end prints exactly one completion line. When nothing failed it is:

```text
sync complete
```

When some work failed - a copy given up on after `--retries-copy` tries, a displacement that could not be made, or a state file that could not be written - the line says how many:

```text
sync complete with 2 failures
```

N is the count of those failures, and the word `failures` is used for every N, including 1. The completion line is emitted at every verbosity level. `sync complete` goes with exit code 0; `sync complete with N failures` goes with exit code 2.

Failed file-transfer diagnostics must identify the relative path, the destination peer URL, the failed phase, and the transport error category when available. The failed phase is one of: `read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or `cleanup`.

Example:

```text
transfer failed for kitchensync.exe to sftp://ace@host/path: move_existing_to_swap_old: permission_denied
```

## SWAP Directory

SWAP staging is used for replacing an existing file without losing evidence of an interrupted swap. For a target `<parent>/<basename>`, the SWAP paths are:

- `<parent>/.kitchensync/SWAP/<encoded-basename>/new`
- `<parent>/.kitchensync/SWAP/<encoded-basename>/old`

`<encoded-basename>` is the basename percent-encoded when needed so it can be used as one path segment on every supported transport. SWAP is the only thing KitchenSync keeps outside the root's `.kitchensync/`, and only while a copy into that directory is under way: when the last copy into a directory finishes, its `SWAP/` folder and its `.kitchensync/` folder are removed if they are empty.

A leftover SWAP directory is found by the walk: when a directory's listing shows a `.kitchensync` folder, its `SWAP/` is recovered before the directory's entries are decided (see multi-tree-sync.md, "Reading A Directory").

A SWAP directory counts as empty when it holds only operating-system litter files: names starting with `._` (the AppleDouble files macOS writes beside anything it touches on exFAT and FAT drives) and `.DS_Store`. KitchenSync deletes that litter before removing the directory; any other leftover entry keeps the directory in place.

SWAP is for user files only. `state.txt` and `runs.txt` are replaced by the `.new`/`.old` rule in state.md instead.

## BAK Directory

Displaced entries are recoverable from `<root>/.kitchensync/BAK/<run>/<relpath>` until cleaned, after `--keep-bak-days` days (default: 90). The layout and cleanup are defined in state.md ("BAK"). BAK is on the same filesystem as the entries it holds, so a displacement is one rename. A sync root that spans more than one filesystem (a mount point inside the tree) cannot displace entries on the other filesystem: those displacements fail and are counted.

## Peer Transports

Each peer is reached through filesystem operations selected by URL scheme. `sftp://` URLs use SSH/SFTP. `file://` URLs and bare paths use local filesystem operations. Both schemes must provide the same behavior to the sync engine. A directory listing must return each entry's name, type, size and modification time in as few filesystem calls as the platform allows: on macOS that is one bulk attribute call per directory (`getattrlistbulk`), falling back to a per-entry stat only where the call is unavailable. On an external exFAT drive a per-entry stat costs a disk seek and a round trip through the user-space filesystem driver, tens of milliseconds each. After startup, every root-bound operation receives the connected peer root handle for the winning URL and a path relative to that root.

### Required Operations

Every transport must support:

- `list_dir(peer, path)`: List immediate children: name, `is_dir`, `mod_time`, and `byte_size`. `byte_size` is the file size in bytes for files, or -1 for directories.
- `stat(peer, path)`: Return `mod_time`, `byte_size`, and `is_dir`; or "not found".
- `open_read(peer, path)` -> handle: Open a file for streaming read.
- `read(handle, max_bytes)`: Pull the next chunk; returns bytes or EOF.
- `seek(handle, offset)`: Move the read position to a byte offset, so that the next `read` starts there. Used for the content check in "Moved Files".
- `close_read(handle)`: Close a read handle.
- `open_write(peer, path)` -> handle: Open a file for streaming write, creating the file and parent directories as needed.
- `write(handle, bytes)`: Push the next chunk.
- `close_write(handle)`: Finalize the write: flush and close.
- `rename(peer, src, dst)`: Same-filesystem rename. The destination must not already exist.
- `delete_file(peer, path)`: Remove a file.
- `create_dir(peer, path)`: Create a directory and any needed parents.
- `delete_dir(peer, path)`: Remove an empty directory.
- `set_mod_time(peer, path, time)`: Set file/directory modification time.

`list_dir` returns only regular files and directories. Symbolic links, special files (devices, FIFOs, sockets), and any other non-regular entry types are silently omitted by the implementation. The same applies to `stat`: if the path is a symlink or special file, return "not found."

Streaming and bounded buffering are implemented above scheme-specific filesystem operations. The scheme-specific layer provides the chunk-level read and write primitives.

### Error Semantics

All operations return the same error categories regardless of transport: not found, permission denied, I/O error. Sync logic never matches on transport-specific errors. Network failures (connection drop, timeout) surface as I/O errors - sync logic doesn't distinguish "disk read failed" from "SFTP channel died."

### Testability

The full sync is tested end-to-end via the CLI with mixed peer schemes. Typical end-to-end tests use local peers under a temporary directory; additional tests exercise `sftp://` peers against localhost. See `TESTING-GUIDELINES.md`.

SFTP replacement behavior must be tested against a local SFTP fixture or fake transport that rejects plain rename-over-existing while allowing ordinary create, write, delete, and rename-to-new-path operations. KitchenSync must pass that fixture for both state replacement and user-file replacement by never renaming over an existing file. Tests must not depend on a personal LAN host or external account.

## Errors

- **Argument errors** on non-help invocations (too few peers, multiple `+` peers, invalid settings) -> print to stdout, exit 1
- **No history at the sync root and no canon** -> not an error: print `first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative` and merge additively (see Canon Peer)
- **Unreachable peer** -> skip, log at error level, continue with others
- **Directory listing failure** -> try that listing up to `--retries-list` total times; if it still fails, exclude that peer for that directory subtree without modifying its state lines or peer files under that subtree. If the failed peer is the canon peer (`+`), skip decisions for that directory subtree for all peers
- **Canon peer unreachable** -> exit 1
- **Fewer than two reachable peers** -> exit 1
- **No contributing peer reachable** (every reachable peer is subordinate, whether marked `-` on the command line or auto-subordinated) -> print `No contributing peer reachable - cannot make sync decisions`, exit 1
- **Transfer failure before SWAP `old` exists** -> clean up staging and requeue the copy later if its total try count is below `--retries-copy`; otherwise log final failure and skip the file for this run (re-discovered next run); a copy given up on counts toward the failure count in the completion line
- **Transfer failure after SWAP `old` exists** -> leave SWAP state in place, log error, and recover it before making decisions for that directory again
- **Archive old failure** (cannot rename SWAP `old` to BAK after the replacement is in place) -> log error and leave SWAP `old` for later recovery
- **Displacement failure** (cannot rename to BAK/) -> log error and skip the displacement (file remains in place); counts toward the failure count in the completion line
- **SWAP staging failure** (cannot create staging directory or write staging file) -> treat as transfer failure
- **`set_mod_time` failure** (after a completed copy - file is already in place) -> log at error level; the copy is not undone. The destination peer's state line already records the winning mod_time, so the discrepancy will be detected and corrected on the next run
- **State write failure** -> log error at error level and leave whatever `state.txt` the peer already had; the next normal run repairs an interrupted replacement at startup (see state.md). Counts toward the failure count in the completion line

## Unicode Normalization

The same visible name can be stored as different bytes. macOS reports an accented letter such as `é` decomposed (NFD: `e` followed by a combining accent), while Linux and Windows usually keep it composed (NFC: one character). KitchenSync compares names after converting them to NFC, so the two forms are one entry, not a file on one peer and a different file on the other. State paths are read and written in NFC (see state.md, "Reading"). Nothing is renamed on disk: an entry keeps the form its filesystem stores, and a new entry is created with its NFC name. If one directory holds two entries whose names differ only in Unicode form (possible only on a filesystem that compares bytes), the NFC one is synced, the other is left alone, and an error line names it.

An NFD name stored on an exFAT drive cannot be opened, renamed or deleted from macOS: the macOS exFAT driver lists it, but looks names up in NFC, and the name hash stored on disk is for the NFD form. Linux stores whatever bytes it is given, so a tool on Linux that copies names from a macOS SFTP listing (which is NFD) can leave such names on a drive. KitchenSync itself creates new entries with NFC names. On macOS the run then prints `listing failed ... No such file or directory` for that path every time. Repair it on Linux by renaming the entry to its NFC form, or on the Mac with `extart/exfat-fix-nfd-names.py`, which rewrites such names in NFC directly on the unmounted volume (its header explains how).

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BAK/.
