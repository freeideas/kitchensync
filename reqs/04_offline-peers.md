# 04_offline-peers: Offline peer behavior

## Behavior

A peer unreachable during a run does not participate in listings or decisions, and its snapshot is not modified. On a subsequent run when it is reachable, discrepancies between its filesystem state and its (unchanged) snapshot drive sync to bring it up to date. Derived from `specs/multi-tree-sync.md` §"Offline Peers" and `specs/sync.md` §"Startup".

## $REQ_IDs
- `04.4` — An unreachable peer's snapshot rows are not modified during the run (`last_seen` is not advanced and `deleted_time` is not set).
- `04.5` — When a previously-unreachable peer is reachable on a subsequent run, sync detects discrepancies between its filesystem and its (still-unchanged) snapshot and brings the peer's state into the group.

## Notes
This is the mechanism behind occasionally-connected peers (USB drives, sleeping laptops): each peer carries its own snapshot history and resumes when reachable. Reachability minimums (`<2 reachable` and canon-unreachable) are in `02_startup-connect.md`.
