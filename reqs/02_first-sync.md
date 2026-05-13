# 02_first-sync: First sync requires a canon peer

## Behavior

The first sync of a group needs an authoritative source because there are no snapshots to distinguish "new" from "deleted". When no reachable peer has snapshot history and no `+` peer is designated, the program prints a specific suggestion and exits without doing any sync work. Derived from `sync.md` §"Canon Peer (+)" and §Startup.

## $REQ_IDs

- `02.1` — When two or more peers are provided and no peer has an existing `.kitchensync/snapshot.db` and no `+` peer is designated, the program prints `First sync? Mark the authoritative peer with a leading +` and exits 1.

## Notes

After the first sync, subsequent runs without `+` use bidirectional logic — see `02_bidirectional-sync.md`. Canon-peer propagation and BAK/ displacement during a first sync with `+` are covered by `03_canon-peer.md`. The snapshot-creation outcome is covered by `02_snapshot-db.md`. Auto-subordination of snapshotless peers is covered in `03_subordinate-peer.md`.
