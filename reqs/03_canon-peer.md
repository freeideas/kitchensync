# 03_canon-peer: Canon peer (`+`) overrides decisions unconditionally

## Behavior

A peer prefixed with `+` is the canon peer for the run. Its state wins all conflicts unconditionally. Canon is required on a first run when no peer has snapshot history; after snapshots exist, runs may proceed without a canon peer. Derived from `./specs/sync.md` (`Canon Peer (+)`) and `./specs/multi-tree-sync.md` (`Decision Rules` — `With a canon peer (+)`).

## $REQ_IDs
- `03.1` — On a first run where no peer has a `.kitchensync/snapshot.db` and no peer is prefixed `+`, the program prints `First sync? Mark the authoritative peer with a leading +` and exits 1.
- `03.2` — When canon has a file and another peer does not, the file is copied to the other peer.
- `03.3` — When canon lacks a file that another peer has, the other peer's file is displaced to BAK/.
- `03.4` — When canon and another peer disagree on file contents, canon's version overwrites the other peer (without using mod_time).
- `03.5` — Subsequent runs without `+`, after snapshots exist, proceed bidirectionally and do not require canon.

## Notes

The error case where canon is unreachable is captured under `02_peer-connection` (req 02.27). Type-conflict resolution under canon is captured under `03_type-conflicts` (reqs 03.41, 03.42).
