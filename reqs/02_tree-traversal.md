# 02_tree-traversal: Multi-tree combined walk

## Behavior

Sync proceeds by a single recursive combined-tree walk. At each directory level, peers are listed concurrently, the union of entry names is decided, actions are applied, and the walk recurses pre-order into surviving directories. A `list_dir` failure on one peer excludes that peer from the affected subtree without touching its snapshot; if every contributing peer fails at a directory, the subtree is skipped entirely. Derived from `specs/multi-tree-sync.md` §"Overview" / §"Algorithm" and `specs/concurrency.md` §"Directory Listing".

## $REQ_IDs
- `02.13` — At each directory level, listings of all reachable peers are issued concurrently, not in a sequential await loop.
- `02.14` — Traversal is pre-order: every entry in a directory is decided and acted on before recursing into any subdirectory.
- `02.15` — When a directory is displaced on a peer, the walk does not recurse into that directory on that peer.
- `02.16` — If `list_dir` fails on a reachable peer at a path, that peer is excluded from decisions for that directory and its entire subtree.
- `02.17` — If all contributing peers fail listing for a directory, that directory and its entire subtree are skipped — no decisions, no subordinate displacement, no snapshot updates.
- `02.18` — When `list_dir` fails on a reachable peer at a directory, that peer's snapshot rows for that directory's subtree are not modified during the run.
- `02.46` — A `list_dir` failure on a reachable peer is logged at `error` level.

## Notes
Built-in excludes (`.kitchensync/`, symlinks, special files, default `.git/`) are covered in `03_ignore-rules.md`.
