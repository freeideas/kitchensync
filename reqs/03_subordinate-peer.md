# 03_subordinate-peer: Subordinate peer (`-`) receives outcomes without voting

## Behavior

A peer prefixed with `-` is subordinate: its files are not considered when deciding the authoritative state, but it is brought into conformance with the resulting decision. Any peer that lacks a `snapshot.db` is automatically subordinate (unless it is canon). Derived from `./specs/sync.md` (`Subordinate Peer (-)`, `Startup` step 6) and `./specs/multi-tree-sync.md` (`Subordinate Peers`).

## $REQ_IDs
- `03.11` — A `-`-prefixed peer's existing files do not influence which version other peers end up with.
- `03.12` — A subordinate peer that has a file the contributing peers do not have has that file displaced to BAK/.
- `03.13` — A subordinate peer that lacks a file the contributing peers have receives a copy of that file.
- `03.14` — A subordinate peer's `snapshot.db` is updated and uploaded back at the end of the run.
- `03.15` — A peer that has no `.kitchensync/snapshot.db` is treated as subordinate even without an explicit `-` prefix (unless it is the `+` canon peer).
- `03.16` — On a subsequent run where the same peer is invoked without `-`, it participates in decisions normally using its uploaded snapshot history.
- `03.17` — A `-` prefix on a peer that already has a snapshot still suppresses that peer's contribution to decisions for that run.
