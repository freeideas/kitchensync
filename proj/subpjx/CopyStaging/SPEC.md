# CopyStaging:

## Purpose

CopyStaging owns peer mutations that move user file content during a sync run.
It accepts copy and displacement work chosen by traversal, performs those
operations through the shared peer transport surface, protects existing user
files with SWAP and BAK staging, emits copy and delete progress, and reports
the operation results that allow the caller to update snapshots.

This child is not an executable. The root coordinator or traversal child
provides reachable peer handles, run options, relative paths, selected source
files, destination peers, winning file metadata, verbosity, and dry-run state.
CopyStaging does not decide which file or directory should win. It applies the
already chosen outcome.

## Responsibilities

CopyStaging exposes a queue boundary for file copies. Each queued copy is one
transfer from one source peer path to one destination peer path. The queued
copy stores its own try count. `--retries-copy N` allows at most `N` total tries
for that queued copy, including the first try. A failure before SWAP `old`
exists increments only that queued copy's try count. If tries remain, the copy
moves behind other queued work. If no tries remain, the copy is failed for the
run and an error-level diagnostic is emitted.

CopyStaging starts copy work incrementally. Once traversal enqueues eligible
work from an early directory, workers may begin transfers while later
directories are still unscanned. Each transfer acquires one global copy slot
before it starts reading or writing file content. The default slot maximum is
10. `--max-copies N` sets the maximum to `N`. The slot count is global across
all source and destination scheme combinations, and there is no observable
per-peer, per-host, or per-connection copy limit below that global maximum.
Directory listing, snapshot download, snapshot upload, directory creation, BAK
cleanup, TMP cleanup, and SWAP cleanup never consume copy slots.

At `trace` verbosity, CopyStaging emits a plain stdout line each time a copy
slot is acquired and each time one is released:

```text
copy-slots active=<n>/<max>
```

The `active` value is the global active file-copy count after the acquire or
release event. It does not describe network connections.

CopyStaging transfers file content with bounded buffering. The total active
buffer storage for a transfer is independent of the file size. A successful
transfer writes the selected source file bytes to the destination file and sets
the destination modification time to the winning modification time supplied by
the sync decision. If setting the modification time fails after the copied file
is live, CopyStaging does not undo the copied file; it emits an error-level
diagnostic and reports the failure so the next run can rediscover the mismatch.

For an existing user-file replacement, CopyStaging never depends on
rename-over-existing behavior. Before changing the live destination path, it
recovers or fails any existing SWAP directory for the target basename. It then
writes the replacement bytes to:

```text
<target-parent>/.kitchensync/SWAP/<encoded-basename>/new
```

If the live target file exists, it moves that file to:

```text
<target-parent>/.kitchensync/SWAP/<encoded-basename>/old
```

Only after that move succeeds does it rename `new` to the live target path.
After the replacement is live, it sets the winning modification time, moves
`old` to BAK when `old` exists, and removes the empty SWAP directory for that
basename. SWAP uses `new` and `old` as the only live staging path names for a
user-file replacement. The `<encoded-basename>` path segment is supplied by the
format rules and percent-encodes the target basename when needed so the value
is one path segment on every supported transport.

CopyStaging preserves recoverability on replacement failures. If moving the
live destination to SWAP `old` fails, the original live destination remains in
place and CopyStaging removes that transfer's SWAP `new` staging when removal
is possible. If a transfer fails before SWAP `old` exists, the same cleanup and
retry rules apply. If a failure happens after SWAP `old` exists, CopyStaging
leaves the SWAP state for a later normal run and emits an error-level
diagnostic. If moving SWAP `old` to BAK fails after the replacement is live,
CopyStaging leaves SWAP `old` for later recovery and emits an error-level
diagnostic. Failure to create or write TMP or SWAP staging is treated as a
transfer failure.

CopyStaging exposes user-file SWAP recovery for one peer at one directory
before that directory's live user entries are listed for decisions. In a normal
run, each direct child of `.kitchensync/SWAP/` at that directory level is
recovered for the corresponding target basename in the same parent directory:

- If `old` exists and the live target exists, delete `new` if present, move
  `old` to BAK, and remove the empty SWAP directory.
- If `old` exists, `new` exists, and the live target is missing, move `new` to
  the live target, move `old` to BAK, and remove the empty SWAP directory.
- If `old` exists, `new` is missing, and the live target is missing, move `old`
  back to the live target and remove the empty SWAP directory.
- If `old` is missing, `new` exists, and the live target exists, delete `new`
  and remove the empty SWAP directory.
- If `old` is missing, `new` exists, and the live target is missing, move
  `new` to the live target and remove the empty SWAP directory.

If user-file SWAP recovery fails for a peer at a directory, CopyStaging reports
that failure so traversal can skip sync decisions for that peer's current
directory subtree and leave its snapshot rows unchanged for that subtree.

CopyStaging exposes inline displacement to BAK for one peer path. Before the
rename, it creates `<parent>/.kitchensync/BAK/<timestamp>/` and any missing
parents. It moves a displaced file to
`<parent>/.kitchensync/BAK/<timestamp>/<basename>`. It moves a displaced
directory to the same shape as one directory tree. If the displacement fails,
the original live path remains in place, an error-level diagnostic is emitted,
and the caller is told that no snapshot deletion update is allowed for that
peer path.

CopyStaging performs all BAK displacement cases requested by traversal:
deletion decisions for files and directories, subordinate live files or
directories that no contributing peer has live or in snapshot rows,
canon-vs-wrong-type conflicts, contributing file-vs-directory conflicts where
the file wins, and subordinate wrong-type paths after contributing peers choose
the outcome. Displacement is inline work during traversal, not queued copy
work, and it does not acquire a copy slot.

CopyStaging exposes BAK and TMP cleanup for one peer at one visited directory
level in a normal run. It checks that directory's `.kitchensync/` metadata
directory, removes `.kitchensync/BAK/<timestamp>/` directories older than
`--keep-bak-days`, and removes `.kitchensync/TMP/<timestamp>/` directories
older than `--keep-tmp-days`. The defaults are 90 days for BAK and 2 days for
TMP. Cleanup age is determined from the `<timestamp>` path segment. Cleanup
leaves BAK and TMP timestamp directories that are not older than the configured
retention value and never removes `.kitchensync/SWAP/` directories by age.

CopyStaging creates TMP staging for temporary metadata and cleanup work under:

```text
.kitchensync/TMP/<timestamp>/
```

BAK and TMP timestamp directory names use `YYYY-MM-DD_HH-mm-ss_ffffffZ`.
Timestamp creation and parsing come from FormatRules. Concurrent TMP staging
paths must not collide across transfers.

CopyStaging owns copy and delete progress lines for the user-file actions it
performs. At `info`, `debug`, and `trace` verbosity, it emits plain stdout
lines in action order. At `error` verbosity, it suppresses these progress
lines. A copy progress line is `C <relpath>`. A delete or displacement progress
line is `X <relpath>`. `<relpath>` is the slash-separated relative path from
the sync root. Copy progress emits one line per copied path regardless of how
many peers receive that path. Delete progress emits one line per deleted path
regardless of how many peers displace that path. CopyStaging emits no progress
line for directory creation, directory listing, snapshot work, or BAK/TMP
cleanup. Progress is plain lines only and is identical whether stdout is a
terminal or a pipe.

CopyStaging's transfer diagnostics identify the transfer relative path, the
destination peer URL, the failed phase, and the transport error category when
one is available. The failed phase is one of `read_source`, `write_swap_new`,
`move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or
`cleanup`. Transfer failure after SWAP `old` exists, archive-old failure,
displacement failure, exhausted copy tries, and set_mod_time failure are
error-level diagnostics.

## Boundaries

CopyStaging does not parse command-line arguments, validate option values,
select fallback URLs, connect peers, decide reachability, decide first-sync
rules, print help, print the final completion line, or choose stdout versus
stderr policy for the whole process. Its output obligations are limited to the
copy, delete, copy-slot, and operation-failure lines assigned to this child.

CopyStaging does not decide reconciliation outcomes. It does not classify peer
state, compare modification times for conflict winners, apply excludes, walk
the combined tree, retry directory listings, or choose source and destination
peers. Traversal supplies that work.

CopyStaging does not own SQLite schema, local snapshot database lifecycle,
snapshot download or upload, snapshot SWAP recovery, snapshot row updates, or
stale snapshot-row cleanup. It reports copy and displacement outcomes so the
snapshot child or caller can update rows only after the corresponding peer
mutation has succeeded.

CopyStaging does not implement local filesystem or SFTP behavior directly. All
peer reads, writes, stats, renames, deletes, directory creation, and
modification-time writes go through PeerTransportSurface. CopyStaging must not
match on transport-specific errors and must not depend on any transport
renaming over an existing destination.

CopyStaging does not own dry-run policy. When the run coordinator supplies
dry-run mode, this child may exercise queueing, slot acquisition, source reads,
retry accounting, and progress output as directed by the dry-run child, but it
must leave peer writes, SWAP, BAK, TMP, destination modification times, cleanup,
and displacement disabled according to that policy.

Its invariants are:

- No more than the configured global maximum number of transfers hold active
  copy slots at any time.
- Active-copy accounting is global across all peer schemes and independent of
  directory listings, snapshot operations, directory creation, and cleanup.
- Each destination copy is one source peer path to one destination peer path
  with independent try accounting.
- User-file replacement writes SWAP `new` before moving the live target, and
  moves the live target to SWAP `old` before renaming `new` live.
- Once SWAP `old` exists, failures preserve recoverable SWAP state for a later
  normal run.
- Displacement to BAK is a same-peer rename and leaves the original live path
  in place when the rename fails.
- Successful existing-file replacement leaves the replacement bytes and winning
  modification time at the live path and makes the replaced file recoverable in
  BAK.
- BAK and TMP cleanup are retention checks on timestamp-named metadata
  directories and never age-delete SWAP.
