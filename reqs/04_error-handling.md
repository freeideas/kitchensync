# 04_error-handling: Runtime error behavior

## Behavior

The program tolerates some runtime errors (unreachable peer, transfer failure) by logging and continuing, but aborts the run for others (canon unreachable, fewer than two reachable peers, all reachable peers subordinate). Per-directory listing errors exclude one peer from a subtree without affecting others. Derived from `sync.md` §Startup / §Errors and `multi-tree-sync.md` §Algorithm / §"Offline Peers".

## $REQ_IDs

- `04.7` — A peer that cannot be reached on any of its URLs is skipped with a warning at `error` verbosity, and the run continues with the remaining reachable peers.
- `04.8` — When fewer than two peers are reachable after connection attempts, the program exits 1.
- `04.9` — When the `+` canon peer is unreachable, the program exits 1.
- `04.10` — When every reachable peer is subordinate after auto-subordination, the program prints `No contributing peer reachable — cannot make sync decisions` and exits 1.
- `04.11` — A `list_dir` failure on one peer at one directory excludes that peer from decisions for that directory and its entire subtree without affecting other peers.
- `04.12` — A file transfer failure is logged at `error` verbosity and that file is skipped for the run; other transfers continue.
- `04.13` — A displacement failure (cannot rename to BAK/) is logged at `error` verbosity and the displacement is skipped, leaving the file in place.
- `04.14` — A `set_mod_time` failure after a successful copy is logged at `error` verbosity; the copy is not undone, and the discrepancy is corrected on the next run.
- `04.15` — When a displacement failure occurs as part of a file copy sequence, the associated copy is also skipped and its TMP staging file is removed.
- `04.16` — An unreachable peer's snapshot rows are not modified during the run.
- `04.17` — A snapshot-download failure other than a clean "not found" (e.g., I/O error, permission denied) treats the peer as unreachable: a warning is logged at `error` verbosity, the peer is excluded from the reachable set, and the reachable-count and canon-reachability checks are re-evaluated against the updated set.
- `04.18` — A snapshot-upload failure is logged at `error` verbosity and the run completes normally; the peer's existing `.kitchensync/snapshot.db` is left untouched and the staging file under `.kitchensync/TMP/` is retained for `--xd` cleanup.
- `04.19` — When every contributing peer fails to list a directory, that directory and its entire subtree are skipped: no decisions are made, no entries are processed, and no subordinate-peer files at or below that level are displaced.
- `04.20` — When `list_dir` fails on a peer at a directory, the snapshot rows for that peer in the affected subtree are not modified during the run.
- `04.21` — Failure to create a TMP staging directory or write the TMP staging file is treated as a transfer failure for that file.
