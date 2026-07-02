# 012_peer-roles: Canon, subordinate, new, and offline peer roles

## Behavior
This concern derives from `specs/sync.md` sections "Canon Peer (+)",
"Subordinate Peer (-)", and "Startup", plus `specs/multi-tree-sync.md`
sections "Subordinate Peers" and "Offline Peers". It covers how peer roles are
assigned, how snapshotless peers become subordinate, what canon and subordinate
mean during a run, how subordinate snapshots are still maintained, how a peer
can later become contributing, and how unreachable peers are excluded without
snapshot modification.

## $REQ_IDs

- `012.1` -- A reachable non-canon peer whose `.kitchensync/snapshot.db` did not exist on disk at startup is treated as subordinate for that run.
- `012.2` -- A reachable canon peer whose `.kitchensync/snapshot.db` did not exist on disk at startup remains a contributing peer for that run.
- `012.3` -- A reachable peer marked with `-` is treated as subordinate for that run even when it has snapshot history.
- `012.4` -- If no peer in the reachable set has snapshot data and no canon peer is designated, KitchenSync prints `First sync? Mark the authoritative peer with a leading +`.
- `012.5` -- If no peer in the reachable set has snapshot data and no canon peer is designated, KitchenSync exits 1.
- `012.6` -- If no contributing peer is reachable after automatic subordination, KitchenSync prints `No contributing peer reachable - cannot make sync decisions`.
- `012.7` -- If no contributing peer is reachable after automatic subordination, KitchenSync exits with an error.
- `012.8` -- A run with reachable snapshot history on at least one contributing peer does not require a canon peer.
- `012.9` -- A canon peer's state wins sync conflicts unconditionally.
- `012.10` -- During decision selection, subordinate peer entries do not contribute to sync decisions.
- `012.11` -- After a sync decision is selected, subordinate peers receive the selected outcome.
- `012.12` -- A normal run writes updated snapshot data back to subordinate peers.
- `012.13` -- A peer that was subordinate in a previous normal run participates as a contributing peer on a later run when it is reachable, has snapshot history, and is not marked with `-`.
- `012.14` -- A peer that is unreachable at startup and does not cause startup to exit is omitted from all listings for that run.
- `012.15` -- A peer that is unreachable at startup and does not cause startup to exit is omitted from all sync decisions for that run.
- `012.16` -- A peer that is unreachable at startup and does not cause startup to exit has no snapshot rows modified during that run.
- `012.17` -- When a peer that was unreachable in one run is reachable on a later run, discrepancies between its filesystem state and its existing snapshot rows drive sync decisions.

## Notes
This file defines peer roles. The detailed file and directory outcomes that use
those roles belong to `013_file-decisions.md` and
`014_directory-and-type-decisions.md`.
