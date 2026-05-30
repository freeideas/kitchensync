# 017_peer-roles-and-startup-state: Peer roles and startup decision state

## Behavior
This concern derives from `specs/sync.md` sections "Canon Peer (`+`)", "Subordinate Peer (`-`)", "Startup", and "Errors", `specs/multi-tree-sync.md` sections "Subordinate Peers" and "Decision Rules", and `specs/README.md` sections "First Sync" and "Add A Peer". It covers canon peer authority, explicit subordinate peers, automatic subordination of snapshotless non-canon peers, first-sync behavior when no peer has snapshot history, no-contributing-peer startup failure, and subordinate peers receiving group outcomes without contributing to decisions during that run.

## $REQ_IDs
- `017.1` -- A reachable canon peer is treated as a contributing peer even when its `.kitchensync/snapshot.db` did not exist on disk at startup.
- `017.2` -- When a canon peer is reachable, KitchenSync uses the canon peer as the authoritative source of truth for sync decisions.
- `017.3` -- A subordinate peer does not contribute live entries or snapshot history to sync decisions during that run.
- `017.4` -- A subordinate peer remains eligible to receive file and directory operations that apply the selected group outcome during that run.
- `017.5` -- A reachable non-canon peer whose `.kitchensync/snapshot.db` did not exist on disk at startup is automatically treated as subordinate during that run.
- `017.6` -- A peer that was subordinate in an earlier run contributes to decisions in a later run when it is run without a subordinate prefix and has snapshot history.
- `017.7` -- When no reachable peer has snapshot data and no canon peer is designated, KitchenSync prints `First sync? Mark the authoritative peer with a leading +`.
- `017.8` -- When no reachable peer has snapshot data and no canon peer is designated, KitchenSync exits 1.
- `017.9` -- When no reachable peer has snapshot data and a canon peer is designated, KitchenSync does not print the first-sync canon requirement message for that condition.
- `017.10` -- When no contributing peer is reachable after explicit and automatic subordinate roles are applied, KitchenSync prints `No contributing peer reachable - cannot make sync decisions`.
- `017.11` -- When no contributing peer is reachable after explicit and automatic subordinate roles are applied, KitchenSync exits 1.

## Notes
This category owns how peer roles determine which reachable peers can vote and which peers only receive outcomes. Prefix parsing belongs to `003_peer-addressing`; peer reachability belongs to `004_peer-connectivity`; snapshot existence detection and empty local snapshot creation belong to `006_snapshot-lifecycle`; per-path file, directory, and type-conflict decision rules belong to `008_decision-making`; the mechanics of making a subordinate peer match the outcome belong to copy, displacement, and snapshot-update categories.
