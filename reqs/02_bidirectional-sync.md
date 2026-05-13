# 02_bidirectional-sync: Bidirectional sync without a canon peer

## Behavior

Once every reachable peer has a snapshot from a previous run, the program syncs bidirectionally without a `+` (canon) peer — snapshots distinguish "new" from "deleted" so no single peer needs to be authoritative. This is the positive complement of the first-sync error. Derived from `README.md` §"Next Time" and `sync.md` §"Canon Peer (+)" / §Startup.

## $REQ_IDs

- `02.6` — When every reachable peer has an existing `.kitchensync/snapshot.db` and no `+` peer is designated, the sync runs to completion and exits 0.

## Notes

Per-entry propagation (newest-wins, deletion vs existing, directory existence) is covered in `03_decision-rules.md` and `03_directory-decisions.md`. The negative case where a first sync requires a canon peer is in `02_first-sync.md`. Snapshot persistence after a run is covered in `02_snapshot-db.md` and `02_combined-tree-walk.md`.
