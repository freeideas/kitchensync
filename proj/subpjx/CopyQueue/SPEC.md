# CopyQueue:

## Purpose

CopyQueue owns queued user-file transfers for one KitchenSync run. It accepts
file-copy work as soon as traversal discovers it, starts eligible transfers
while later directories are still being scanned, enforces the one global active
copy limit, retries failed copies according to each copy's own try count, and
replaces destination user files through recoverable SWAP staging.

The child operates on already connected peers. A queued transfer is one source
peer and relative source file path, one destination peer and relative
destination file path, the slash-separated user relative path for reporting,
the winning modification time selected by the sync decision, and the winning
byte size selected by the sync decision.

## Responsibilities

CopyQueue exposes a run-scoped queue operation. The caller supplies the maximum
active copy count, the maximum total tries per queued copy, connected peer
handles, a dry-run mutation policy, and an event sink for structured copy
events. If the caller does not supply a maximum active copy count, the queue
uses `10`. If the caller supplies `--max-copies N`, the queue uses `N`.

CopyQueue exposes an enqueue operation that can be called while traversal is
still running. Enqueueing a copy makes it eligible for workers immediately when
a global copy slot is available; the queue must not wait for the whole tree to
be scanned before starting copy work. The queue also exposes a drain operation
that waits until traversal has closed the queue and every queued copy has
either succeeded, been skipped for this run, or reached its copy-try limit.

CopyQueue enforces one active file-copy limit across the whole run. A transfer
holds one global copy slot from the moment its try starts until the try has
finished all required cleanup for that try. These transfers all count against
the same limit:

- `file://` source to `file://` destination.
- `file://` source to `sftp://` destination.
- `sftp://` source to `file://` destination.
- `sftp://` source to `sftp://` destination.

The queue does not count directory listing, snapshot download, snapshot upload,
directory creation, BAK cleanup, TMP cleanup, or SWAP cleanup as active file
copies, even when those operations run during the same overall sync run.
CopyQueue does not impose any per-peer, per-host, or per-connection active-copy
limit below the global active copy limit.

Each queued copy owns its own failed-copy try count. The first try counts
toward `--retries-copy`. When a try fails before the copy has reached its total
try limit, CopyQueue increments only that queued copy's count, moves that copy
behind other queued copy work, releases its slot after required cleanup, and
continues other queued work in the same run. When a copy reaches its total try
limit, CopyQueue marks that copy failed for this run and does not requeue it.
The same try rules apply to local, SFTP, and mixed-scheme copies.

Before starting replacement for a destination user path, CopyQueue derives the
destination basename and percent-encodes it when needed so the encoded value is
one path segment on every supported transport. For target
`<target-parent>/<basename>`, it uses these paths:

- SWAP new:
  `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new`
- SWAP old:
  `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old`

Before writing replacement content for that target, CopyQueue recovers any
existing SWAP directory for the encoded basename, or treats recovery failure as
a failed copy try before SWAP old exists.

Each normal transfer follows this order:

1. Acquire one global copy slot.
2. Recover or fail the destination SWAP directory for the encoded basename.
3. Stream source file content into SWAP `new`.
4. If the destination has an existing file at the final target path, rename
   that file to SWAP `old`.
5. Rename SWAP `new` into the final target path.
6. Set the final destination file modification time to the winning
   modification time from the sync decision.
7. If SWAP `old` exists, archive it to
   `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>`.
8. Remove the empty SWAP directories for that transfer.
9. Release the global copy slot.

A destination that had no existing file creates no BAK entry for that
destination path. A local-to-local copy may use a native filesystem copy
primitive to populate SWAP `new`, but it must not write replacement content
directly to the final destination path.

Active transfers stream content with bounded buffering. CopyQueue starts
writing to the destination SWAP `new` file while it reads from the source and
must not buffer the entire source file in memory before destination writing
begins. The total buffer memory used by one active transfer is fixed by the
implementation's chosen buffer sizes and is independent of the copied file
size.

CopyQueue reports structured events for copy start, copy-slot acquire,
copy-slot release, transfer success, transfer skip, and transfer failure. Slot
events include the current active count and the global maximum. Transfer
failure events include the relative path, destination peer identity, transport
error category when available, and one failed phase:
`read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`,
`set_mod_time`, `archive_old`, or `cleanup`.

When a transfer fails before the existing destination has been moved to SWAP
`old`, CopyQueue deletes that transfer's SWAP `new` file when possible before
releasing the copy slot. It then applies the normal retry rule: requeue behind
other work if tries remain, otherwise mark the copy failed for this run.

When moving an existing destination file to SWAP `old` fails, the original
destination must remain in place. CopyQueue deletes SWAP `new` when possible,
releases the copy slot, reports the `move_existing_to_swap_old` failure, and
skips that copy for the rest of the run instead of requeueing it.

When a transfer fails after the existing destination has been moved to SWAP
`old` and before replacement fully completes, CopyQueue leaves the peer-visible
SWAP state in place. In particular, SWAP `old` remains durable evidence of an
incomplete KitchenSync replacement rather than a user deletion. The next
recovery pass is responsible for repairing that state before the directory is
used for sync decisions.

If archiving SWAP `old` fails after SWAP `new` has already become the final
destination, CopyQueue reports the `archive_old` failure and leaves SWAP `old`
for later recovery. If setting the final modification time fails after the file
is in place, CopyQueue reports the `set_mod_time` failure and does not undo the
replacement.

For every BAK archive path it creates during queued replacement, CopyQueue asks
the snapshot child for a fresh process-local timestamp string and uses that
string as the `<timestamp>` directory component.

## Boundaries

CopyQueue does not decide which files should be copied, which peer is canon or
subordinate, which paths are excluded, which directories are traversed, or
which entries should be displaced. Tree planning supplies copy work that is
already eligible to run.

CopyQueue does not connect peers, choose fallback URLs, authenticate SFTP,
normalize command-line URLs, or implement scheme-specific filesystem calls. It
uses the transport child for stat, streaming read, streaming write, rename,
delete, directory creation, empty-directory deletion, and modification-time
operations.

CopyQueue does not own traversal-wide SWAP recovery, snapshot SWAP recovery,
BAK/TMP cleanup, or inline displacement of entries selected for deletion or
type-conflict removal. It uses the staging recovery child for destination
SWAP recovery and nearby BAK archive mechanics needed by a queued replacement.

CopyQueue does not update snapshot rows. On transfer success or installed-file
failure states, it returns structured results so the caller can apply the
snapshot rules owned by the snapshot child.

CopyQueue does not format stdout and does not decide verbosity. It emits
structured copy and slot events; the output child decides which events become
stdout lines.

CopyQueue must preserve these invariants:

- active file copies across the whole run never exceed the configured global
  maximum;
- no extra per-peer, per-host, or per-connection copy limit is imposed below
  that global maximum;
- every queued copy tracks tries independently;
- retryable failed copies move behind other queued copy work;
- copies that reach their total try limit are not requeued in the same run;
- replacement content reaches the final destination only through SWAP `new`;
- an existing destination is moved to SWAP `old` before SWAP `new` is moved
  into the final path;
- a failed move to SWAP `old` leaves the original destination in place and
  skips that copy for the rest of the run;
- a failure after SWAP `old` exists leaves SWAP state visible for recovery;
- a successful transfer removes its empty SWAP directories;
- active transfer buffer memory is independent of source file size.
