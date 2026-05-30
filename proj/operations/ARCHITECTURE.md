# operations Architecture

## Scope

`operations` owns peer-side mutation sequences after sync traversal has already
decided what should happen. It composes connected `TransportHandle` operations
into safe, recoverable filesystem effects:

- copy replacement through user-entry SWAP staging;
- traversal-time recovery of interrupted user-entry SWAP replacements;
- displacement of existing entries to nearby BAK names;
- destination directory creation;
- BAK and TMP retention cleanup;
- suppression of peer-side mutations in dry-run mode.

The module does not choose sync outcomes, decide which paths should exist,
schedule copy retries, enforce active-copy limits, store snapshot rows, connect
peers, recover snapshot databases, or render progress. Those responsibilities
remain with sibling modules through root-owned contracts.

## Public Surface

The root-visible API is an `OperationExecutor` contract consumed by `sync` and
`runtime`. It should stay behavioral and narrow:

- `recover_directory_swaps(peer, directory)` scans only the user-entry SWAP
  children relevant to a traversal directory and completes or rolls back
  interrupted replacements according to transport-observable state.
- `displace_to_bak(peer, path, timestamp)` moves an existing user entry out of
  the active tree to a nearby BAK path when traversal has selected a
  displacement outcome.
- `create_directory(peer, path)` creates a selected destination directory and
  any missing parents.
- `cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)`
  removes expired BAK and TMP timestamp directories for the current traversal
  directory.
- `execute_copy_attempt(source_peer, source_path, destination_peer,
  destination_path, winning_meta)` performs one scheduled copy attempt and
  returns a `CopyResult` with the failing `TransferPhase` and `TransportError`
  category when the attempt cannot complete.

The executor receives `RunConfig`, `PeerSession`, `RelPath`, `EntryMeta`,
`Timestamp`, `TransportHandle`, `TransportError`, `DiagnosticSink`, and
`ProgressSink` values from the root contracts. It must not expose
transport-specific error types or implementation details from local filesystem
or SFTP backends.

## Internal Design

The implementation should remain a leaf module. No generated child modules are
needed unless future source adds independently visible operation families. The
implementation can still be organized into private Rust files or private
helpers around the operation sequences below, but those helpers are not sibling
APIs.

`OperationExecutor` is the coordinating facade. It applies dry-run handling
before destination-side mutations, then delegates to small private routines for
SWAP, BAK, directory, retention, and bounded stream-copy work. Private routines
share only sequence helpers such as path construction, encoded-basename
handling, best-effort cleanup, timestamp acquisition, and phase-tagged error
mapping.

Important private responsibilities:

- SWAP naming and state inspection: build paths under
  `<parent>/.kitchensync/SWAP/<encoded-basename>/`, where the basename is
  percent-encoded when needed so it is valid as one segment on every supported
  transport. The snapshot module's `.kitchensync/SWAP/snapshot.db/` area is not
  owned or recovered here.
- Copy replacement sequencing: stream source data to SWAP `new`, move an
  existing destination aside to SWAP `old` when present, rename SWAP `new` into
  final position, set modification time from `winning_meta`, archive SWAP `old`
  to nearby BAK with a fresh `Timestamp`, and remove empty staging directories
  when possible.
- BAK displacement: create the nearby
  `<parent>/.kitchensync/BAK/<timestamp>/` directory and rename the existing
  user file or directory into it without deciding whether displacement is
  appropriate.
- Directory creation: create only the directory requested by traversal and its
  missing parents, then report normalized transport failures.
- Retention cleanup: list BAK and TMP timestamp directories, compare the
  timestamp path segment against the configured retention window, and delete
  only expired operation-owned timestamp directories.
- Dry-run wrapper: suppress every peer-side mutation while preserving the
  result shape callers need for planning, diagnostics, retry accounting, and
  traversal.

## Data Flows

### Copy Attempt

`runtime` schedules one copy attempt and calls `operations`. `operations` reads
from the source peer transport and mutates only the destination peer transport.
Content transfer uses bounded buffering whose total buffer size is independent
of file size and begins writing before the whole source is buffered. Local
`file://` to local `file://` transfers may use a host copy primitive to
populate SWAP `new`, but they must preserve the same SWAP, final rename,
modification-time, BAK, and cleanup behavior as other transports.

The normal sequence is:

1. Determine the destination parent and basename.
2. Recover or fail any existing user-entry SWAP directory for that basename.
3. Write replacement content to
   `<parent>/.kitchensync/SWAP/<encoded-basename>/new`.
4. If the destination has a file at the final path, rename it to SWAP `old`.
5. Rename SWAP `new` to the final path using a non-overwriting rename.
6. Set the destination modification time to the winning modification time.
7. If SWAP `old` exists, rename it to
   `<parent>/.kitchensync/BAK/<fresh-timestamp>/<basename>`.
8. Remove empty SWAP directories created for the transfer when possible.

Failures are mapped to these root `TransferPhase` values:

- `read_source`: source open or stream read failed.
- `write_swap_new`: destination SWAP `new` creation or write failed.
- `move_existing_to_swap_old`: moving an existing final file aside failed.
- `rename_final`: moving SWAP `new` to the final path failed.
- `set_mod_time`: applying the winning modification time failed.
- `archive_old`: moving SWAP `old` to BAK failed.
- `cleanup`: removing no-longer-needed staging state failed.

If source reading or SWAP `new` writing fails before SWAP `old` exists, the
module best-effort deletes SWAP `new` and empty staging directories, then
returns a pre-old transfer failure. If moving the existing destination to SWAP
`old` fails, the original destination remains in place and the attempt returns a
terminal failure for this run. If any later phase fails, the module leaves the
observable SWAP or final-path state in place for later recovery and reports the
failed phase. A `set_mod_time`, `archive_old`, or final `cleanup` failure must
not roll back a copied final file.

The returned `CopyResult` describes only the attempt outcome. Retry policy,
attempt counting, copy-slot accounting, and final scheduling state stay in
`runtime`. Snapshot row mutation stays in `sync` and `snapshot`.

### SWAP Recovery During Traversal

When traversal enters a directory, `sync` asks `operations` to recover user
entry SWAP state for that directory before making decisions that depend on
active user paths. `operations` inspects direct children of
`<directory>/.kitchensync/SWAP/`, excluding `snapshot.db`, and applies the
specified state machine for each target basename:

- `old` plus target exists: move `old` to nearby BAK and remove the empty SWAP
  directory.
- `old`, `new`, and no target: rename `new` to the target, move `old` to
  nearby BAK, and remove the empty SWAP directory.
- `old` only: rename `old` back to the target and remove the empty SWAP
  directory.
- `new` plus target, with no `old`: delete `new` and remove the empty SWAP
  directory.
- `new` only: rename `new` to the target and remove the empty SWAP directory.

Recovery uses the same nearby BAK pattern as displacement. If recovery for any
swap directory fails, the failed SWAP directory is left in place and the result
is a directory-recovery failure for that peer and directory. The caller treats
that as a listing failure for the current directory subtree.

### Displacement To BAK

`sync` decides that an active entry should be displaced and passes the exact
peer path and timestamp to `operations`. `operations` creates
`<parent>/.kitchensync/BAK/<timestamp>/` and renames
`<parent>/<basename>` to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
A directory displacement is a single same-filesystem rename that preserves the
whole subtree under BAK; this module must not recurse into that directory or
split displacement into per-child operations.

If displacement fails, the entry remains in place, an error result is returned,
and snapshot update decisions stay with the caller.

### Directory Creation

`sync` requests creation only after deciding a directory should exist. The
operation creates the requested directory and any missing parents through the
peer transport and returns the normalized result. Parent discovery, tree
traversal order, and snapshot updates are outside this module.

### Retention Cleanup

Traversal asks `operations` to clean retention areas for the current directory
after sync processes that directory's entry union. `operations` checks
`.kitchensync/BAK/<timestamp>/` and `.kitchensync/TMP/<timestamp>/` under that
directory, determines age from the timestamp path segment, and deletes expired
timestamp directories according to `keep_bak_days` and `keep_tmp_days`.

Cleanup must not purge `.kitchensync/SWAP/` by age. SWAP state is handled only
by explicit recovery rules. Cleanup failures are nonfatal operation failures
reported with the peer, directory, cleanup target when known, and normalized
transport error category; they must not change user-entry sync decisions that
have already been processed.

### Dry Run

In dry-run mode, `recover_directory_swaps`, `displace_to_bak`,
`create_directory`, and `cleanup_retention` return planned/no-op results
without creating, modifying, renaming, deleting, archiving, cleaning up, or
setting modification times through any peer URL. Dry-run copy attempts still
open and read the source file through the same bounded-buffer path used by
normal transfers, and read failures are reported as `read_source`.
Destination-side write, SWAP, final rename, BAK, delete, cleanup, and
`set_mod_time` phases are planned but not executed.

The module does not print the required dry-run stdout phrase. Runtime or root
output owns that observable text.

## Dependencies

`operations` depends on root contracts and on the transport API provided by
connected `PeerSession` values. It may consume diagnostics and progress sinks,
but rendering and verbosity filtering remain in `runtime`.

Allowed dependencies:

- root configuration and value contracts: `RunConfig`, `RelPath`, `EntryMeta`,
  `Timestamp`, `TransferPhase`, `CopyResult`, and retention settings;
- connected peer access: `PeerSession` and its `TransportHandle`;
- normalized failures: `TransportError`;
- output channels: `DiagnosticSink` and `ProgressSink`.

Disallowed dependencies:

- direct SQLite or snapshot schema access;
- snapshot SWAP recovery or upload responsibilities;
- sync decision internals or combined-tree traversal state beyond the specific
  operation request;
- runtime scheduler state such as retry counters or active-copy slots;
- transport implementation internals from local filesystem or SFTP backends;
- CLI parsing or process-exit behavior.

## Error And Recovery Model

Every transport failure crossing the module boundary must be expressed as a
root `TransportError` category plus the operation phase or operation context
that failed. Copy failures use the exact `TransferPhase` values required by the
root contract. Cleanup attempts may be best-effort, but their failures must not
hide the original failed phase.

Operation sequences should be written so that interruption leaves enough
observable state for a later traversal recovery pass to complete the user path
or restore the original entry. SWAP, BAK, and TMP paths are operation-owned
artifacts; their naming, retention, and cleanup rules belong inside this
module, except for snapshot database SWAP paths owned by `snapshot`.

Transport behavior must remain abstract. The module may choose an optimized
local-to-local content copy path when both peers use `file://`, but observable
state transitions and error categories must remain the same for `file://`,
`sftp://`, and mixed transfers.

## Visibility

No `operations` type should become a root contract unless at least one sibling
module needs to name it. Root-visible contracts should be limited to the
executor methods, request/result types shared by `sync`, `operations`, and
`runtime`, and phase/error reporting values. Internal path builders,
encoded-basename helpers, recovery classifiers, cleanup routines, timestamp
selection, and dry-run wrappers should remain private to this module.
