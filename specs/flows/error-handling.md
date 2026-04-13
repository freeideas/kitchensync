# Error Handling

Error scenarios and recovery behaviors across all components.

## Argument Errors

No peers, multiple `+` peers, invalid option values -> print error + help text to stdout, exit 1.

## Startup Errors

- **No snapshots and no canon** (multi-peer mode) -> print suggestion to use `+`, exit 1
- **Canon peer unreachable** -> exit 1
- **No peers reachable** -> exit 1
- **No contributing peer reachable** (multi-peer mode) -> exit 1

## Runtime Errors

- **Unreachable peer** -> skip, log warning, continue with others
- **Only one reachable** (multi-peer mode) -> log warning, run in snapshot-only mode for that peer

## Transfer Errors

- **Transfer failure** -> log, skip file (re-discovered next run)
- **TMP staging failure** -> treat as transfer failure

## Walk Errors

- **Listing failure for a peer at a directory** -> exclude that peer from the entire subtree, log error, continue with remaining peers
- **Displacement failure** -> log error, skip (file remains). If part of a copy sequence, skip the copy too (clean up TMP). For directories: exclude the peer from recursion and do not cascade tombstones -- the snapshot is left unchanged so the next run re-attempts deletion. Copies already enqueued to the failed peer for entries within the failed subtree will fail individually and are handled by normal transfer failure logic (clean up TMP, log, skip)
- **.syncignore read failure** -> log at warn, proceed with parent-level rules only -- do not skip the directory

## Snapshot Errors

- **Snapshot download failure** (corrupt, permission denied, I/O) -> log warning, treat as new peer, create empty snapshot locally. If not canon: auto-subordinate
- **Snapshot upload failure** -> log error, leave TMP for `--xd` cleanup

## Exit Codes

- **0** -> at least one sync completed successfully, or single-peer snapshot completed
- **1** -> fatal error (no peers reachable, canon unreachable, argument error, instance lock conflict)
