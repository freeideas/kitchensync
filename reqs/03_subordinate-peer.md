# 03_subordinate-peer: Subordinate peer behavior

## Behavior

A subordinate peer (prefixed with `-`, or any peer without an existing snapshot file) is listed and receives outcomes but does not contribute to decisions. After decisions are made the subordinate peer is brought into conformance with the group: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it, and its snapshot is still downloaded, updated, and uploaded. Derived from `sync.md` §"Subordinate Peer (-)" and `multi-tree-sync.md` §"Subordinate Peers".

## $REQ_IDs

- `03.21` — A subordinate peer's file states do not influence decisions made by other peers.
- `03.22` — Files present on a subordinate peer but not in the group's decided state are displaced to BAK/ on the subordinate peer.
- `03.23` — Files in the group's decided state but missing from a subordinate peer are copied to that peer.
- `03.24` — A peer with no existing `.kitchensync/snapshot.db` is treated as subordinate even without the `-` prefix, unless it is the `+` canon peer.
- `03.25` — A subordinate peer's `.kitchensync/snapshot.db` is still updated and uploaded back at the end of the run.
- `03.26` — Re-running the same peer without `-` on a later run lets it participate as a normal bidirectional peer using its snapshot history.
- `03.27` — More than one subordinate peer per run is allowed.

## Notes

The error case where every reachable peer turns out to be subordinate after auto-subordination is in `04_error-handling.md`.
