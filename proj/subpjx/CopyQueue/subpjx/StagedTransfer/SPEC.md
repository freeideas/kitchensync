# StagedTransfer:

## Purpose

StagedTransfer owns one attempted file copy after QueueRunner has granted a
copy slot. It derives the destination SWAP paths, recovers any existing SWAP
state for the destination basename, streams the source file into SWAP `new`,
replaces the final destination through SWAP `old`, applies the winning
modification time, archives the displaced file when one exists, cleans the
empty SWAP directories after success, and returns a phase-specific result.

This child performs the work for one try only. It does not decide when the try
starts, how many other transfers may run, whether the copy should be retried,
or when an exhausted copy becomes a run failure.

## Responsibilities

StagedTransfer exposes an operation that runs one copy try. The caller supplies
the connected source peer, connected destination peer, relative source file
path, relative destination file path, slash-separated user path for reporting,
winning modification time, winning byte size, a timestamp generator for BAK
paths, and the peer file operations needed for the try. Those file operations
must cover existence checks, streaming read, streaming write to a new path,
rename to a missing path, delete, directory creation, empty-directory removal,
and modification-time updates. The caller also supplies the operation that
recovers an existing user-data SWAP directory for the encoded target basename.

For target `<target-parent>/<basename>`, StagedTransfer percent-encodes
`<basename>` when needed so the encoded value is one path segment on every
supported transport. It derives exactly these replacement paths:

- SWAP new:
  `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new`
- SWAP old:
  `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old`

Before writing replacement content, StagedTransfer recovers any existing SWAP
directory for that encoded basename. If recovery fails, the try fails before
SWAP `old` exists and no replacement begins.

For a normal try, StagedTransfer performs these phases in order:

1. Recover existing SWAP state for the target basename.
2. Stream source file content into SWAP `new`.
3. If the destination target currently has a file, move that file to SWAP
   `old`.
4. Move SWAP `new` into the final destination path.
5. Set the final destination file modification time to the winning
   modification time.
6. If SWAP `old` exists, archive it to
   `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>`.
7. Remove the empty SWAP directories for this transfer.

StagedTransfer writes replacement content only to SWAP `new` before the final
rename. A local-to-local copy may use an efficient local copy primitive to
populate SWAP `new`, but it must not write replacement content directly to the
final destination path.

StagedTransfer replaces existing destination files using the SWAP sequence even
on transports whose `rename(src, dst)` rejects an existing destination. It
moves the existing destination to SWAP `old` before moving SWAP `new` into the
final destination path. It never relies on rename-over-existing behavior for
the final user path.

Active transfer I/O is streaming. StagedTransfer starts writing destination
SWAP `new` while reading from the source and must not buffer the entire source
file before destination writing begins. The total buffer memory used by one
try is fixed by chosen buffer sizes and is independent of the copied file
size.

When the destination had no existing file, StagedTransfer moves SWAP `new` into
place, sets the modification time, removes empty SWAP directories after
success, and creates no BAK entry for that destination path.

When the destination had an existing file, StagedTransfer archives SWAP `old`
only after SWAP `new` has become the final destination. It asks the supplied
timestamp generator for the `<timestamp>` component and archives the old file
to `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>`.

StagedTransfer returns one of these try outcomes:

- success, after the final file is in place, the winning modification time has
  been set, any SWAP `old` has been archived, and empty SWAP directories for
  the transfer have been removed;
- skip for the rest of the run, only when moving the existing destination to
  SWAP `old` fails and the original destination remains in place;
- failure, with the failed phase set to one of `read_source`,
  `write_swap_new`, `move_existing_to_swap_old`, `rename_final`,
  `set_mod_time`, `archive_old`, or `cleanup`.

If a failure occurs before the existing destination has been moved to SWAP
`old`, StagedTransfer deletes that transfer's SWAP `new` file before returning
when possible. It reports the failed phase so QueueRunner can apply its retry
rule.

If moving the existing destination to SWAP `old` fails, StagedTransfer leaves
the original destination in place, deletes SWAP `new` when possible, and
returns the skip result for the rest of the run with phase
`move_existing_to_swap_old`.

If a failure occurs after the existing destination has been moved to SWAP `old`
and before replacement fully completes, StagedTransfer leaves the peer-visible
SWAP state in place. SWAP `old` remains visible as incomplete-replacement state
for later recovery.

If setting the final modification time fails after SWAP `new` has become the
destination, StagedTransfer reports `set_mod_time` failure and does not undo
the replacement. If archiving SWAP `old` fails after the replacement is in
place, StagedTransfer reports `archive_old` failure and leaves SWAP `old` for
later recovery. If final SWAP cleanup fails after all replacement and archive
work succeeded, StagedTransfer reports `cleanup` failure.

## Boundaries

StagedTransfer does not own queue scheduling, global copy-slot accounting,
per-copy try counts, retry ordering, or exhausted-try decisions. QueueRunner
owns those concerns and calls this child once for each granted try.

StagedTransfer does not decide which files should be copied, which peer is
canon, which peer is subordinate, which paths are excluded, which directories
are traversed, or which entries should be displaced for type conflicts. Its
caller supplies copy work that is already eligible.

StagedTransfer does not connect peers, parse command-line URLs, authenticate
SFTP, or implement scheme-specific filesystem calls. It runs against
caller-supplied peer file operations for existence checks, streaming read,
streaming write, rename, delete, directory creation, empty-directory deletion,
and modification-time updates.

StagedTransfer does not own traversal-wide SWAP recovery, snapshot SWAP
recovery, TMP staging, BAK/TMP age cleanup, or type-conflict displacement. It
uses only the caller-supplied user-data SWAP recovery operation and the BAK
archive path it derives for this one replacement try.

StagedTransfer does not update snapshot rows and does not format stdout. It
returns structured outcomes and phase information so its caller can update
snapshot state and route user-visible output through the output owner.

StagedTransfer must preserve these invariants:

- replacement content reaches the destination only through SWAP `new`;
- an existing destination is moved to SWAP `old` before SWAP `new` is moved
  into the final path;
- a failed move to SWAP `old` leaves the original destination in place and
  skips that copy for the rest of the run;
- a failure before SWAP `old` exists removes SWAP `new` when possible before
  the copy slot is released by the caller;
- a failure after SWAP `old` exists leaves SWAP state visible for recovery;
- a successful transfer removes its empty SWAP directories;
- a first-time destination creates no BAK entry;
- active transfer buffer memory is independent of source file size.
