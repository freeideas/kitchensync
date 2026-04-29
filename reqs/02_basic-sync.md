# 02_basic-sync: Two-peer bidirectional sync of files

## Behavior

The core flow: given two peers, propagate new files, propagate modifications (newest-wins), and propagate deletions (using snapshots from a previous run). Files unchanged on both sides require no copy. Derived from `./specs/sync.md` (`Run`, `Operation Queue`, `File Copy`) and `./specs/multi-tree-sync.md` (`Algorithm`, `Decision Rules` 1–3).

## $REQ_IDs
- `02.51` — On a first run with `+peerA peerB` where only `peerA` has a file, the file is copied to `peerB`.
- `02.52` — On a first run with `+peerA peerB` where only `peerB` has a file, the file is removed from `peerB` (canon peerA does not have it).
- `02.53` — When both peers already hold an identical file with matching mod_time and byte_size, no copy is performed and the snapshot row is created/updated for that entry.
- `02.54` — Given snapshots from a prior run, modifying a file on one peer causes that peer's newer version to overwrite the other peer on the next run.
- `02.55` — Given snapshots from a prior run, deleting a file on one peer causes the other peer's copy to be displaced to BAK/ on the next run (deletion propagates).
- `02.56` — A new file added on either peer between runs is copied to the other peer on the next run, regardless of which peer originated it.
- `02.57` — When a file copy is performed, the destination's modification time is set to the winning entry's mod_time (the decision-time `mod_time`, not re-read from the source after copying).
- `02.58` — Adding a third peer (no existing snapshot) to a previously-synced pair brings the new peer fully into conformance with the group's state on the next run.
