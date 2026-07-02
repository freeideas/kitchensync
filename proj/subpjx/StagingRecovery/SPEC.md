# StagingRecovery:

## Purpose

StagingRecovery owns peer-side user-data staging state that must be repaired or
removed during the combined-tree walk. It recovers interrupted user-file SWAP
directories before a directory is listed for sync decisions, moves displaced
user entries into nearby BAK storage, creates TMP staging paths for temporary
work, and removes stale BAK and TMP timestamp directories after each directory
level has been processed.

This child performs peer mutations only when the caller invokes its operations
for a normal run. It reports failures as operation results; stdout formatting,
retry policy, dry-run suppression, sync decisions, and snapshot row changes
belong to other children or to the root coordinator.
It uses TransportOperations for the peer filesystem operations needed to list,
stat, create, rename, and delete paths.

## Responsibilities

StagingRecovery exposes an operation to recover user-data SWAP state for one
peer and one parent directory before that parent directory is listed. The
operation checks `<parent>/.kitchensync/SWAP/` directly, even though
`.kitchensync/` is excluded from sync decisions. If the SWAP directory is
missing, recovery succeeds without changing user data. If it exists, each direct
child is treated as the encoded basename for one target entry in the same
parent directory and is recovered before live entries from that parent are used
for sync decisions.

StagingRecovery also exposes an operation to recover the one user-data SWAP
directory for a target `<parent>/<basename>` before another child starts
replacement of that path. If recovery for that encoded basename does not
succeed, the operation reports failure and the caller must not start the
replacement.

For a target `<parent>/<basename>` and SWAP directory
`<parent>/.kitchensync/SWAP/<encoded-basename>/`, user-data recovery applies
these cases:

- If `old`, `new`, and the target all exist, leave the target in place, delete
  `new`, move `old` to
  `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, and remove the empty SWAP
  directory.
- If `old` and the target both exist while `new` is missing, leave the target
  in place, move `old` to
  `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, and remove the empty SWAP
  directory.
- If `old` and `new` both exist while the target is missing, rename `new` to
  the target path, move `old` to
  `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, and remove the empty SWAP
  directory.
- If `old` exists while `new` and the target are both missing, rename `old`
  back to the target path and remove the empty SWAP directory.
- If `new` and the target both exist while `old` is missing, leave the target
  in place, delete `new`, and remove the empty SWAP directory.
- If `new` exists while `old` and the target are both missing, rename `new` to
  the target path and remove the empty SWAP directory.

If any user-data SWAP recovery step fails for a peer at a directory level,
StagingRecovery returns a failed-listing result for that peer and directory.
The caller must then treat that peer's live listing for the current directory
as failed and leave that peer's snapshot rows for the current directory subtree
unchanged. SWAP state that was not fully recovered remains in place until a
later recovery succeeds.

StagingRecovery exposes a displacement operation for one existing entry
`<parent>/<basename>` on one peer. Before the move, it creates
`<parent>/.kitchensync/BAK/<timestamp>/` and any missing parents. It then
renames the original entry to
`<parent>/.kitchensync/BAK/<timestamp>/<basename>`. On success, the original
path is absent. If the displaced entry is a directory, the directory is moved as
one entry and its subtree is preserved below the BAK destination. BAK
destinations are always under the displaced entry's own parent directory, never
under a root-level aggregate BAK directory.

StagingRecovery exposes a TMP staging-path operation for transfer work. For the
directory level supplied by the caller, it creates or returns a staging path
under `<parent>/.kitchensync/TMP/<timestamp>/` that includes the transfer UUID
as a path segment. TMP staging paths are for temporary work and must not replace
or rename over a live user path.

StagingRecovery exposes a cleanup operation for one peer and one parent
directory after the caller has processed the union of live entry names at that
directory level. The operation checks `<parent>/.kitchensync/` directly as
metadata, not as sync input. It lists only the `BAK/` and `TMP/` subdirectories
for cleanup:

- Remove `.kitchensync/BAK/<timestamp>/` directories older than
  `--keep-bak-days`.
- Leave `.kitchensync/BAK/<timestamp>/` directories that are not older than
  `--keep-bak-days`.
- Remove `.kitchensync/TMP/<timestamp>/` directories older than
  `--keep-tmp-days`.
- Leave `.kitchensync/TMP/<timestamp>/` directories that are not older than
  `--keep-tmp-days`.

Cleanup determines age from the `<timestamp>` component of each BAK or TMP
timestamp directory. It does not purge `.kitchensync/SWAP/` by age.

## Boundaries

StagingRecovery does not choose which peers are active, which paths are
excluded, which entries should be copied, which entries should be displaced, or
which child directories should be traversed. It does not own directory-listing
retry policy; it only reports SWAP recovery failure in the form the traversal
owner must treat as a listing failure for that peer and directory.

StagingRecovery does not update snapshot rows. Successful displacement and
failed-listing outcomes are reported to callers so the snapshot owner can apply
or skip row changes according to its own rules.

StagingRecovery does not recover `.kitchensync/SWAP/snapshot.db/` state and
does not upload or download `.kitchensync/snapshot.db`. Snapshot SWAP recovery
belongs to the snapshot owner.

StagingRecovery does not format progress or error output. It returns operation
results with enough context for the output owner to print diagnostics.

StagingRecovery does not define the product-wide timestamp generator or UUID
generator. Callers supply timestamp strings in the specified
`YYYY-MM-DD_HH-mm-ss_ffffffZ` format for BAK and TMP directories and supply the
transfer UUID used in TMP staging paths.

StagingRecovery does not sync metadata directories. Its direct reads and writes
under `.kitchensync/` are limited to SWAP recovery, BAK displacement, TMP path
creation, and BAK/TMP cleanup.
