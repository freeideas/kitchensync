# 03_subordinate-peer: Subordinate peer (`-`) and auto-subordination

## Behavior

A subordinate peer is excluded from decisions but receives the outcome — extras are displaced, missing entries are copied in, directories are reshaped to match. Subordinate status is declared via the `-` prefix, and any peer without an existing `snapshot.db` is automatically treated as subordinate (unless it carries `+`). Derived from `specs/sync.md` §"Subordinate Peer" and `specs/multi-tree-sync.md` §"Subordinate Peers".

## $REQ_IDs
- `03.4` — A `-` prefix on a peer argument marks that peer as subordinate.
- `03.5` — Multiple `-`-prefixed peers are permitted in a single run.
- `03.6` — A subordinate peer's entries are excluded from decisions.
- `03.7` — Files that a subordinate peer has but the group's decision does not retain are displaced to BAK/ on that peer.
- `03.8` — Files the group has but a subordinate peer lacks are copied to the subordinate peer.
- `03.9` — Directories on a subordinate peer that the group's decision does not retain are displaced to BAK/ on that peer.
- `03.10` — Any peer with no existing `snapshot.db` is automatically treated as subordinate (unless it has the `+` prefix).
- `03.11` — A subordinate peer's snapshot is still downloaded, updated during traversal, and uploaded back; on a subsequent run without `-` the peer participates normally using its snapshot history.

## Notes
The `+`/`-` prefix on a bracketed fallback group is covered in `03_fallback-urls.md`. Directory creation/displacement on subordinate peers also follows `03_directory-decisions.md`.
