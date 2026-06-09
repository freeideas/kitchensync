# 006_run-lifecycle: Startup orchestration and run phases

## Behavior
This concern derives from `specs/sync.md` sections "Startup" (steps 2-7),
"Run", and "Operation Queue" (the no-loading-phase ordering), plus
`specs/multi-tree-sync.md` section "Offline Peers".

It covers the overall control flow of a run after the command line is parsed:
connecting all peers in parallel, computing the reachable set, and the
reachability exit conditions - fewer than two reachable peers exits with error,
an unreachable canon (`+`) peer exits with error, no snapshots plus no canon
prints the first-sync suggestion and exits 1, and no contributing peer after
auto-subordination prints `No contributing peer reachable - cannot make sync
decisions` and exits 1. It covers the ordered run phases: the combined-tree
walk, waiting for all enqueued copies to finish, writing updated snapshots back
to peers, disconnecting all peers, logging completion, and exiting 0. It also
covers that unreachable (offline) peers are excluded entirely and their snapshot
rows are left unmodified.

Per-URL connection selection and root creation are
`005_connection-establishment`. Snapshot download, SWAP recovery, and writeback
mechanics are `016_snapshot-storage`. Auto-subordination of snapshotless peers
is `007_peer-roles`. The walk itself is `008_traversal`. Opportunistic row
purging is `018_snapshot-maintenance`. The completion message and exit-code
diagnostics are emitted per `023_logging`.

## $REQ_IDs

- `006.1` -- Connection attempts to all peers proceed concurrently rather than strictly one peer after another.
- `006.2` -- When fewer than two peers are reachable, KitchenSync exits 1.
- `006.3` -- When the canon (`+`) peer is unreachable, KitchenSync exits 1.
- `006.4` -- When no reachable peer has snapshot data and no canon (`+`) peer is designated, KitchenSync prints `First sync? Mark the authoritative peer with a leading +`.
- `006.5` -- When no reachable peer has snapshot data and no canon (`+`) peer is designated, KitchenSync exits 1.
- `006.6` -- When no contributing (non-subordinate) peer is reachable after auto-subordination, KitchenSync prints `No contributing peer reachable - cannot make sync decisions`.
- `006.7` -- When no contributing peer is reachable after auto-subordination, KitchenSync exits 1.
- `006.8` -- Copy work for an already-scanned directory begins while traversal continues into later directories, with no phase that scans the whole tree before any copy starts.
- `006.9` -- All enqueued file copies complete before the run exits.
- `006.10` -- In a normal run, updated snapshots are written back to peers before the run exits.
- `006.11` -- A run that completes all phases exits 0.
- `006.12` -- An unreachable peer is excluded entirely from the run's listings and sync decisions.
- `006.13` -- An unreachable peer's snapshot rows are left unmodified by the run.

## Notes

Bullets 006.4 and 006.6 assert the user-visible output that each lifecycle exit
condition produces; `023_logging` owns general logging format and verbosity, so
these condition-specific messages are not duplicates and stay with the exit
condition that triggers them.
