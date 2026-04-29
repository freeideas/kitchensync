# 03_directory-handling: Directory existence-based decisions and recursion

## Behavior

Directory decisions ignore mod_time and use only existence (in listings and snapshots). Created on peers that lack them, displaced (with their entire subtree) on peers that should not have them. Traversal is pre-order: a directory marked for displacement is moved before any of its descendants are visited. Derived from `./specs/multi-tree-sync.md` (`Algorithm`, `Directory Decisions`, deletion paragraph) and `./specs/sync.md` (`Run` step 2).

## $REQ_IDs
- `03.31` — When any contributing peer has a directory and another peer lacks it, the missing directory is created on the peers that lack it.
- `03.32` — When all contributing peers have removed a directory (tombstones in their snapshots), the directory is displaced to BAK/ on any peer that still has it.
- `03.33` — A directory displaced on a peer is moved by a single rename, taking its full subtree with it (no separate per-file deletion pass on that subtree).
- `03.34` — Recursion descends only into peers that are keeping the directory (peers whose copy is being displaced are not recursed into).
- `03.35` — Directory mod_time is recorded in the snapshot row but is not used to choose between peers when deciding the directory's fate.
- `03.36` — When canon has the directory, it is created on every other reachable peer; when canon lacks it, the directory is displaced on every peer that has it.
