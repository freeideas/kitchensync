# SwapRecovery:

## Purpose

SwapRecovery repairs interrupted user-data SWAP directories for one peer and
one parent directory before that peer's live entries are listed for sync
decisions. It is the staging recovery operation that makes the current
directory safe to inspect after an earlier replacement was interrupted.

The operation works only on user entry SWAP state under
`<parent>/.kitchensync/SWAP/`. If recovery cannot complete for that peer and
parent directory, it reports the peer's current directory listing as failed so
the caller can skip listing results and preserve snapshot rows for that
directory subtree.

## Responsibilities

SwapRecovery exposes an operation to recover user-data SWAP state for a
supplied peer, parent directory, and BAK timestamp. The caller invokes this
operation during normal traversal before listing live entries from the same
peer and parent directory.

The operation checks `<parent>/.kitchensync/SWAP/` directly even though
`.kitchensync/` is not sync input. If that SWAP directory is absent, recovery
succeeds without changing user data. If it exists, the operation lists its
direct children and treats each direct child as the encoded basename for the
corresponding target entry in the same parent directory. For each child
`<encoded-basename>`, the target is `<parent>/<basename>`, the SWAP old path is
`<parent>/.kitchensync/SWAP/<encoded-basename>/old`, and the SWAP new path is
`<parent>/.kitchensync/SWAP/<encoded-basename>/new`.

For each user-data SWAP child, SwapRecovery applies these cases:

- If `old` and the target both exist, leave the target in place, move `old` to
  `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, and remove the empty SWAP
  child directory.
- If `old` and `new` both exist while the target is missing, rename `new` to
  the target path, move `old` to
  `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, and remove the empty SWAP
  child directory.
- If `old` exists while `new` and the target are both missing, rename `old`
  back to the target path and remove the empty SWAP child directory.
- If `new` and the target both exist while `old` is missing, leave the target
  in place, delete `new`, and remove the empty SWAP child directory.
- If `new` exists while `old` and the target are both missing, rename `new` to
  the target path and remove the empty SWAP child directory.

When recovery moves an `old` entry to BAK, the destination is always under the
same parent directory as the target:
`<parent>/.kitchensync/BAK/<timestamp>/<basename>`. The operation must create
the needed BAK parent directories before the move.

The operation reports success only after every direct SWAP child has been
handled and each completed child directory has been removed. On any
filesystem, path decoding, listing, existence check, rename, delete, directory
creation, or cleanup failure during recovery for the supplied peer and parent,
it reports a failed-listing result for that peer and parent directory.

When SwapRecovery reports failure, the caller must treat that peer's listing
for the current directory as failed and must leave that peer's snapshot rows
for the current directory subtree unchanged. SwapRecovery itself does not
delete unrecovered SWAP directories as cleanup; SWAP directories left by
interrupted work remain until a later SWAP recovery succeeds.

## Boundaries

SwapRecovery does not list live user entries for sync decisions. It only runs
before that listing and returns whether listing may proceed for the supplied
peer and parent directory.

SwapRecovery does not decide which peers or parent directories traversal visits.
It does not choose timestamps; callers supply the timestamp string used for BAK
destinations.

SwapRecovery does not update snapshot rows, create snapshot tombstones, or
decide snapshot preservation policy. It returns the failure result that tells
the traversal and snapshot owners to keep the current directory subtree rows
unchanged.

SwapRecovery does not recover `.kitchensync/SWAP/snapshot.db/` state for the
product snapshot database. Snapshot database SWAP recovery belongs to the
snapshot owner.

SwapRecovery does not perform age-based cleanup of SWAP, BAK, or TMP
directories. It removes only the per-entry SWAP child directory after that
child's user-data recovery has completed.

SwapRecovery does not format output, retry failed operations, suppress writes
for dry-run mode, or pick the transport implementation. Its boundary is the
single peer-directory recovery operation and its success or failed-listing
result.
