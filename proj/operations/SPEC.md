# operations:

## Purpose

Own peer-side mutation sequences other than abstract sync decision-making. The module composes connected transport operations into recoverable user-file SWAP replacement, inline displacement to nearby BAK, directory creation, traversal-time user-entry SWAP recovery, BAK and TMP retention cleanup, and dry-run suppression of peer-side mutations.

The module does not decide which paths should exist, schedule copy retries or active-copy slots, store snapshot data, connect peers, or render progress.

## Responsibilities

Expose an `OperationExecutor` interface over connected peer sessions and normalized relative paths. The interface provides at least:

- `recover_directory_swaps(peer, directory)`;
- `displace_to_bak(peer, path, timestamp)`;
- `create_directory(peer, path)`;
- `cleanup_retention(peer, directory, now, keep_bak_days, keep_tmp_days)`;
- `execute_copy_attempt(source_peer, source_path, destination_peer, destination_path, winning_meta)`.

All operations use the peer's already-selected transport handle and root-relative `RelPath` values. The module must preserve the transport abstraction: it may branch on peer capability only for allowed implementation choices, such as local-to-local content copy optimization, but observable behavior and error categories must remain the same for `file://`, `sftp://`, and mixed transfers.

### User-File Copy Attempts

For each copy attempt, write replacement content to the destination peer before touching any existing destination entry. The normal sequence is:

1. Determine the destination parent and basename.
2. Use `<parent>/.kitchensync/SWAP/<encoded-basename>/new` as SWAP `new`.
3. If the destination already has a file at the final path, rename it to `<parent>/.kitchensync/SWAP/<encoded-basename>/old`.
4. Rename SWAP `new` to the final path.
5. Set the destination modification time to the winning modification time supplied with the copy task.
6. If SWAP `old` exists, rename it to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
7. Remove empty SWAP directories created for the transfer when possible.

`<encoded-basename>` is the target basename percent-encoded when needed so it is valid as one path segment on every supported transport. The BAK timestamp is a fresh `Timestamp` value supplied by or obtained through the root timestamp contract; operations must not reuse a run-start timestamp for BAK directory names.

Content transfer must use bounded buffering whose total buffer size is independent of file size. The module must begin writing streamed content before the whole source file is buffered. For local `file://` to local `file://` transfers, the implementation may use a host filesystem copy primitive to populate SWAP `new`, but it must still preserve the same SWAP `new`, SWAP `old`, final rename, modification-time, BAK archive, and cleanup behavior.

Before replacing a user path, any existing SWAP directory for that destination basename must be recovered or reported as a transfer failure. A plain transport rename must never be used to overwrite an existing final destination.

### Copy Failure Obligations

Copy attempt results must identify the failed transfer phase when a phase fails. Valid phases are `read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, and `cleanup`.

If source reading or writing SWAP `new` fails before SWAP `old` exists, delete SWAP `new` and any empty staging directories for that transfer when possible, then return a pre-old transfer failure. Runtime owns counting the try and deciding whether to requeue.

If moving an existing destination to SWAP `old` fails, the original destination must remain in place. Clean up SWAP `new` and empty staging directories when possible, report phase `move_existing_to_swap_old`, and return a terminal copy failure for this run rather than asking runtime to retry the same copy immediately.

If a transfer fails after SWAP `old` exists, leave the SWAP state in place and report the failed phase. The module must not try to infer user deletion from the missing final path; the durable SWAP `old` state is left for later recovery before that directory is interpreted again.

If SWAP `new` has reached the final path and `set_mod_time` fails, leave the copied file in place and report phase `set_mod_time`. The copy result must allow the caller to treat the file content replacement as completed while preserving the diagnostic obligation.

If archiving SWAP `old` to BAK fails after the replacement is in place, leave SWAP `old` in place and report phase `archive_old`. The copy result must allow the caller to treat the new final file as present while preserving the diagnostic obligation.

If final cleanup of empty SWAP directories fails after the final file is in place, report phase `cleanup` and leave remaining staging state for later recovery or cleanup. Cleanup failure must not remove or roll back the final file.

TMP or SWAP staging failures are transfer failures and must be reported through the same phase/error result path as other copy failures.

### User-Entry SWAP Recovery

`recover_directory_swaps` is called before sync lists a directory for decisions in a normal run. It checks the directory-level `.kitchensync/SWAP/` metadata directory and recovers every direct child swap directory for a user entry in that same parent directory. It does not recover `.kitchensync/SWAP/snapshot.db/`, which belongs to the snapshot module.

For each user-entry SWAP child corresponding to target `<basename>`:

- if `old` exists and the target exists, move `old` to BAK and remove the empty SWAP directory;
- if `old`, `new`, and no target exist, rename `new` to the target, move `old` to BAK, and remove the empty SWAP directory;
- if `old` exists while `new` and the target are missing, rename `old` back to the target and remove the empty SWAP directory;
- if `new` and the target exist while `old` is missing, delete `new` and remove the empty SWAP directory;
- if `new` exists while `old` and the target are missing, rename `new` to the target and remove the empty SWAP directory.

Recovery uses the same nearby BAK location as displacement: `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. If recovery for any swap directory fails, return a directory-recovery failure for that peer and directory. The caller treats that as a listing failure for the current directory subtree; operations must leave the failed SWAP directory in place.

### Displacement To BAK

`displace_to_bak` handles deletions and type-conflict removals requested by sync during the combined-tree walk. Before renaming, create `<parent>/.kitchensync/BAK/<timestamp>/` and any missing parents. Then rename `<parent>/<basename>` to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.

BAK directories are local to the displaced entry's parent directory and are not aggregated at the sync root. A directory displacement is a single same-filesystem rename that preserves the whole subtree under BAK. Operations must not recurse into the displaced directory or split a directory displacement into per-child file operations.

If displacement fails, the entry remains in place, an error result is returned, and the caller can skip the snapshot update for that displacement.

### Directory Creation

`create_directory` creates the requested directory and any missing parents through the connected transport. On success, the caller can confirm the directory as present in that peer's snapshot. On failure, operations returns the normalized transport error and leaves snapshot state decisions to the caller.

### BAK And TMP Retention Cleanup

`cleanup_retention` is run for a directory after sync processes that directory's entry union in a normal run. It checks `.kitchensync/` at the current directory even though `.kitchensync/` is excluded from sync decisions.

Any TMP staging this module creates for temporary metadata or cleanup work must
live under the current directory's `.kitchensync/TMP/<timestamp>/` hierarchy and
must include a distinct UUID component for each transfer or cleanup unit that
needs collision isolation. TMP staging must never replace a live user path and
must use a fresh `Timestamp` value for each new `<timestamp>` directory.

Cleanup purges expired timestamp directories under:

- `.kitchensync/BAK/<timestamp>/` when the timestamp is older than `--keep-bak-days`;
- `.kitchensync/TMP/<timestamp>/` when the timestamp is older than `--keep-tmp-days`.

The age of each candidate is determined from the `<timestamp>` path segment. With default run configuration, BAK cleanup uses 90 days and TMP cleanup uses 2 days. The module must not purge `.kitchensync/SWAP/` directories by age; SWAP directories are recovered by explicit recovery rules only.

Cleanup failures are nonfatal operation failures. They must be reported to the caller with the peer, directory, cleanup target when known, and normalized transport error category, without changing sync decisions for user entries already processed.

### Dry-Run Behavior

In `--dry-run`, operations must not create, modify, rename, delete, displace, archive, clean up, or set modification times through any peer URL.

Dry-run `recover_directory_swaps`, `displace_to_bak`, `create_directory`, and `cleanup_retention` return planned/no-op results without touching peer state. Dry-run copy attempts still open and read the source file with the same bounded-buffer path used by normal transfers, and they report read failures through `read_source`. Destination-side write, SWAP, final rename, BAK, delete, cleanup, and `set_mod_time` phases are planned but not executed.

The module does not print the required `dry run` phrase itself; runtime or root output owns that observable stdout text.

## Boundaries

The operations module consumes `RunConfig`, `PeerSession`, `RelPath`, `EntryMeta`, `Timestamp`, `TransportHandle`, `TransportError`, `DiagnosticSink`, and `ProgressSink` contracts from the root module layer.

It returns structured operation results that let sync update snapshots only after successful inline mutations and let runtime apply copy retry accounting without inspecting transport-specific errors. Transport failures must remain in the normalized root categories: `not_found`, `permission_denied`, and `io_error`.

The module does not:

- parse command-line operands, options, fallback URLs, or excludes;
- connect peers, select fallback URLs, authenticate SFTP, or create peer roots during startup;
- decide whether a file, directory, deletion, canon state, or subordinate conformance outcome should win;
- choose traversal order, listing concurrency, listing retries, or subtree exclusion rules;
- enqueue copy work, enforce `--max-copies`, count copy tries, or decide requeue timing;
- read or write SQLite snapshot rows, tombstone descendants, or upload/download snapshot databases;
- recover or upload `.kitchensync/SWAP/snapshot.db/`;
- render stdout, maintain the live progress screen, or choose verbosity filtering.
