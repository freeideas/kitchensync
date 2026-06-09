# SwapTransfer:

## Purpose

SwapTransfer performs one file copy at a time and makes that copy recoverable.
It never writes a destination in place: every replacement is staged through the
SWAP directory under the target's `.kitchensync/`, so an interruption at any
step leaves a state that a later recovery pass can finish or roll back. It owns
two things: the ordered SWAP replacement sequence that carries one source file
into one destination path, and the five-state SWAP recovery machine that
reconciles a leftover SWAP directory back to a single consistent outcome.

SwapTransfer does not decide which files to copy or which modification time
wins; the caller supplies the source peer and path, the destination peer and
path, and the winning mod_time. It does not own the copy-slot limit or the
per-copy retry count; the sibling scheduler holds those and asks SwapTransfer to
run a single transfer try, reporting success or failure back so the scheduler
can requeue or fail the copy. SwapTransfer carries out one try safely and
reports its outcome.

Every filesystem action -- streaming read and write, rename, delete, stat, set
mod_time, directory create, and the host's native local copy -- is a Transport
primitive supplied for the relevant peer. SwapTransfer orchestrates those calls
and never branches on the peer scheme itself.

## Responsibilities

The operations SwapTransfer exposes across its boundary:

### Perform one transfer

Run a single copy of one source file into one destination path through SWAP
staging:

1. Before staging, recover or fail any existing SWAP directory for the target
   basename, using the recovery operation below (019.8).
2. Stream the source content into the SWAP `new` path
   `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new`, writing the new
   content there before the target is touched (019.1). The stream uses a buffer
   whose total size is independent of the size of the file being copied (020.13)
   and begins writing the destination before the whole source has been read into
   memory (020.14). When both ends are local the host's native copy primitive may
   be used, but the copy still passes through the SWAP `new` path rather than
   writing the destination in place (020.15).
3. When a file already exists at the target path, move it to the SWAP `old` path
   `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old` before swapping in
   the new content (019.2).
4. Rename the SWAP `new` file to the final target path (019.3).
5. Set the destination file's modification time to the winning mod_time supplied
   with the request, not a time re-read from the source (019.4).
6. When SWAP `old` exists after the new file is in place, archive it to BAK
   (019.5). The BAK timestamped path layout belongs to the staging concern;
   SwapTransfer names BAK only as the archive destination.
7. Remove the now-empty SWAP directories after the replacement completes (019.6).

The `<encoded-basename>` SWAP path segment is the target basename
percent-encoded so it forms a single path segment on the transport (019.7).

TMP staging directories this transfer needs are created under `.kitchensync/`
(021.7), and distinct transfers use distinct TMP staging directories so
concurrent transfers do not collide (021.8).

### Recover one SWAP directory

Reconcile a single `.kitchensync/SWAP/<encoded-basename>` directory to one
consistent outcome by the presence of (`old`, `new`, target). In a normal run
this runs before a directory's live entries are listed for sync decisions
(019.14), and the same operation is the "recover or fail" step that precedes a
replacement (019.8). The five states:

- `old` present, target present: move `old` to BAK, remove the now-empty SWAP
  directory (019.15).
- `old` present, `new` present, target missing: rename `new` to the target, move
  `old` to BAK, remove the now-empty SWAP directory (019.16).
- `old` present, `new` missing, target missing: rename `old` back to the target,
  remove the now-empty SWAP directory (019.17).
- `old` missing, `new` present, target present: delete `new`, remove the
  now-empty SWAP directory (019.18).
- `old` missing, `new` present, target missing: rename `new` to the target,
  remove the now-empty SWAP directory (019.19).

When recovery of a SWAP directory fails, SwapTransfer reports that failure so the
caller treats that peer's listing for the current directory as failed and
excludes the peer from that directory subtree (019.20).

## Boundaries

### Error obligations

- On transfer failure before SWAP `old` exists, delete the staged SWAP `new`
  file or directory for that transfer (019.9), then report failure so the
  scheduler can requeue or fail the copy by its try count.
- When moving the existing destination to SWAP `old` fails, leave the original
  destination in place (019.10) and skip the copy for this run (019.11).
- On transfer failure after SWAP `old` exists, leave the SWAP state in place for
  later recovery (019.12).
- When archiving SWAP `old` to BAK fails after the replacement is in place, leave
  SWAP `old` in place for later recovery (019.13).
- When SWAP recovery for a directory fails, report that failure so the caller
  excludes the peer from the directory subtree (019.20).

### Dry-run

In a dry-run, SwapTransfer still exercises the copy machinery while making no
change to any peer:

- A queued copy still reads its source file (024.5).
- No TMP, SWAP, or BAK directory is created on a peer (024.13).
- No destination file is written on a peer (024.14).
- No modification time is set on a peer (024.17).
- Peer-side SWAP recovery during traversal is skipped (019.21, 024.20).

### Invariants

- A destination file is never written in place. Every replacement passes through
  the SWAP `new`/`old` sequence, so an interruption at any step leaves a state
  the recovery machine can finish or roll back.
- After a SWAP directory is reconciled, exactly one of `new`/`old`/target remains
  as the live target and the SWAP directory is empty and removed.
- The total buffer size of one transfer is independent of the size of the file
  being copied, and the destination begins receiving bytes before the whole
  source is read.
- In a dry-run, no peer state is mutated.

### Not in scope

- Choosing which files to copy and which mod_time wins, and inline displacement
  of conflicting entries to BAK, belong to the sync engine; the winning mod_time
  arrives with the request.
- The copy-slot limit, the per-copy retry count, accepting new copies while
  earlier ones run, and the decision to requeue or fail by try count belong to
  the sibling scheduler. SwapTransfer runs one transfer try and reports its
  outcome.
- Aging out BAK and TMP entries by retention limit belongs to the sibling
  cleanup worker; SwapTransfer only archives `old` into BAK as part of a
  replacement and removes empty SWAP directories.
- The per-peer filesystem primitives (streaming read/write, rename, delete,
  stat, set mod_time, directory create, native local copy) belong to the
  transport component; SwapTransfer calls them and never branches on scheme.
- The BAK and TMP timestamped path layout and the timestamp string format belong
  to the staging and timestamp concerns; SwapTransfer references BAK only as the
  archive destination.
- Progress and diagnostic lines are emitted through the output component, not
  written directly; SwapTransfer keeps stderr empty.
