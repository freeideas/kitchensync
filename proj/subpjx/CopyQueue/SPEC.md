# CopyQueue:

## Purpose

CopyQueue executes the file copies that the sync engine decides to perform. It
runs those copies concurrently under one global copy-slot limit shared across
the whole run, retries failed copies up to a per-copy try limit, and makes each
replacement recoverable through SWAP staging. It also owns the TMP/SWAP/BAK
staging areas under each directory's `.kitchensync/`: recovering interrupted
SWAP state during traversal, archiving replaced files to BAK, and purging aged
BAK and TMP entries.

CopyQueue does not decide which files to copy or which modification time wins;
those decisions arrive from the caller as enqueued copy requests. CopyQueue only
carries them out safely and reports progress.

## Responsibilities

### Bounded concurrent execution

- Enforce a single global limit on the number of file copies that hold a slot at
  one time across the whole run, defaulting to 10 when the caller supplies no
  limit. The limit is independent of peer scheme, peer count, and connection
  count.
- Count only file copies against the limit. Directory listing, snapshot
  download/upload, directory creation, and BAK/TMP/SWAP cleanup never consume a
  copy slot and may proceed while the limit is full.
- Accept newly enqueued copies while earlier copies are still running, so copy
  work for an already scanned directory begins while later directories are still
  being scanned. CopyQueue never waits for a whole-tree scan before starting.
- Provide the shared concurrent executor that issues the directory listings for
  all reachable peers at a given directory level at the same time rather than one
  after another. Listings run on this executor without consuming a copy slot, so
  they proceed even while the copy-slot limit is full.

### Per-copy retry tracking

- Treat the copy try limit as the maximum total number of tries for one queued
  copy, counting the first try, defaulting to 3 when the caller supplies no
  limit.
- When a copy try fails before the copy reaches its try limit, move that copy to
  the back of the queue and continue other queued work.
- When a copy's try count reaches the limit, mark it failed for the run and do
  not requeue it.
- Track tries per copy. One copy's failed tries never reduce the tries available
  to another copy. The try limit applies identically to local, SFTP, and
  mixed-scheme copies.

### Transfer mechanics

- Stream each copy using a buffer whose total size is independent of the size of
  the file being copied, and begin writing to the destination before the entire
  source has been read.
- When both ends of a copy are local, the host filesystem's native copy
  primitive may be used, but the copy still goes through the SWAP staging path
  rather than writing the destination in place.

### SWAP-staged replacement

For every copy that would replace an existing destination, follow this ordered
sequence so the replacement is recoverable without renaming over an existing
file:

1. Before starting, recover or fail any existing SWAP directory for the target
   basename.
2. Write the new content to `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new`.
3. If a file already exists at the target path, move it to
   `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old`.
4. Rename the SWAP `new` file to the final target path.
5. Set the destination file's modification time to the winning mod_time supplied
   with the copy request, not a time re-read from the source.
6. If SWAP `old` exists, archive it to BAK.
7. Remove the empty SWAP directories.

The `<encoded-basename>` segment is the target basename percent-encoded so it
forms a single path segment on the transport.

### SWAP recovery during traversal

Before a directory's live entries are listed for sync decisions in a normal run,
recover each `.kitchensync/SWAP/<encoded-basename>` directory according to the
five states of (`old` present, `new` present, target present):

- `old` present, target present: move `old` to BAK, remove the empty SWAP
  directory.
- `old` present, `new` present, target missing: rename `new` to the target, move
  `old` to BAK, remove the empty SWAP directory.
- `old` present, `new` missing, target missing: rename `old` back to the target,
  remove the empty SWAP directory.
- `old` missing, `new` present, target present: delete `new`, remove the empty
  SWAP directory.
- `old` missing, `new` present, target missing: rename `new` to the target,
  remove the empty SWAP directory.

### TMP staging and aged cleanup

- Create TMP staging directories under `.kitchensync/`, one distinct directory
  per transfer so concurrent transfers do not collide.
- During a normal run, after the union of entry names at a directory level is
  processed, inspect each peer's `.kitchensync/` directory directly even though
  the built-in exclude removes `.kitchensync/` from synced listings.
- Remove each `.kitchensync/BAK/<timestamp>/` entry older than the BAK retention
  limit (default 90 days) and each `.kitchensync/TMP/<timestamp>/` entry older
  than the TMP retention limit (default 2 days), judging age from the
  `<timestamp>` component of the directory name. Leave entries not older than
  their limit in place, and never purge SWAP by age.

## Boundaries

### Operations exposed across the boundary

- Enqueue a file copy request. A request carries the source peer and path, the
  destination peer and path, and the winning modification time to set on the
  destination. CopyQueue schedules it under the slot limit and reports the
  per-copy outcome (succeeded or failed-for-the-run after exhausting tries).
- Recover the SWAP state for a directory on a peer before that directory is
  listed. CopyQueue reports whether recovery succeeded; on failure the caller
  treats that peer's listing for the directory as failed and excludes the peer
  from that directory subtree.
- Run BAK/TMP aged cleanup for a directory on a peer.

### Construction and the hidden helpers

- CopyQueue is split internally into private helpers it owns and builds itself:
  the run-global copy scheduler, the SWAP-staged single-file transfer helper, and
  the staging cleanup helper. These helpers are an implementation detail of
  CopyQueue, not part of its public surface.
- The function that creates a CopyQueue instance takes exactly two parameters, the
  shared Transport service and the shared Output service it depends on. It
  constructs its own scheduler, transfer, and staging-cleanup helpers internally;
  a caller hands it only the Transport and Output services and never names,
  imports, or constructs any of those helpers. No parameter or return type of any
  public CopyQueue operation, and no parameter of its constructor other than the
  Transport and Output services, is a type that belongs to the scheduler,
  transfer, or staging-cleanup helper. Those helper types stay entirely behind the
  CopyQueue boundary.

### Error obligations

- On transfer failure before SWAP `old` exists, delete the staged SWAP `new`,
  then requeue or fail the copy by its try count.
- When moving the existing destination to SWAP `old` fails, leave the original
  destination in place and skip the copy for this run.
- On transfer failure after SWAP `old` exists, leave the SWAP state in place for
  later recovery.
- When archiving SWAP `old` to BAK fails after the replacement is in place, leave
  SWAP `old` in place for later recovery.
- When SWAP recovery for a directory fails, report that failure so the caller
  excludes the peer from the directory subtree.
- All progress and diagnostics (the `C`/`X` lines and copy-slot trace events) are
  emitted through the output component, not written directly; CopyQueue keeps
  stderr empty.

### Dry-run

In a dry-run, CopyQueue still exercises the copy machinery so the run is as
realistic as possible, while making no change to any peer:

- Queued copies still read their source files, still acquire copy slots subject
  to the global active-copy limit, and the per-copy try limit still applies the
  same way it does in a normal run.
- No peer state is mutated: no TMP, SWAP, or BAK directories are created on
  peers, no destination file is written, and no modification time is set on a
  peer.
- Peer-side SWAP recovery during traversal and peer-side BAK/TMP cleanup during
  traversal are both skipped.

### Not in scope

- Deciding which files to copy, which mod_time wins, and inline displacement of
  conflicting entries to BAK belong to the sync engine.
- The uniform per-peer filesystem operations (streaming read/write, rename,
  delete, stat, set mod_time, native local copy) belong to the transport
  component; CopyQueue calls them and never branches on scheme itself.
- Snapshot row updates that correspond to a completed copy belong to the snapshot
  component.
- The exact `C`/`X` progress-line and copy-slot trace text, and the embedded
  timestamp string format, are owned by the logging and timestamp concerns.

## Invariants

- At most the configured number of file copies hold a slot at any instant across
  the whole run.
- A destination file is never written in place; every replacement passes through
  the SWAP `new`/`old` sequence so an interruption is always recoverable.
- A copy is tried at most its try-limit times in total, and try budgets are
  independent across copies.
- In a dry-run, no peer state is mutated.
