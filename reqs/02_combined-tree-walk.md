# 02_combined-tree-walk: Recursive combined-tree traversal

## Behavior

A single recursive walk visits the combined tree of all reachable peers. At each directory level, the program lists every peer in parallel, unions the entry names, decides the authoritative state per entry, applies the decision, and recurses into kept directories. The traversal is pre-order: every entry at a level is decided and acted on before any subdirectory is entered. Per-peer snapshot rows are updated as decisions are made during the walk, before the actual file operations execute. Derived from `multi-tree-sync.md` §Overview / §Algorithm / §"Snapshot Updates".

## $REQ_IDs

- `02.26` — The combined-tree walk visits each shared directory only once, even when multiple reachable peers have it.
- `02.27` — At each directory level, the entries from all reachable peers' listings are unioned before per-entry decisions are made.
- `02.28` — After a file copy completes successfully on a destination peer, that peer's snapshot row for the copied entry has its `last_seen` set to the current sync timestamp.
- `02.29` — Traversal is pre-order: at each directory level every entry is decided and acted on before the walk descends into any subdirectory of that level.
- `02.30` — A directory the group decides to keep is recursed into only on peers that keep it; peers that do not keep it are excluded from that subtree's walk.
- `02.31` — Per-peer snapshot rows are updated when decisions are made, before the corresponding file operations (create, displace, copy) execute.
- `02.34` — A peer's snapshot row for an entry that is confirmed present in that peer's listing during traversal has its `last_seen` set to the current sync timestamp.
- `02.35` — When an entry is confirmed absent on a peer with an existing snapshot row whose `deleted_time` is NULL, that row's `deleted_time` is set to the row's existing `last_seen` value.
- `02.36` — When an entry is confirmed absent on a peer whose snapshot row already has `deleted_time` set, the row is left unchanged.
- `02.37` — After `create_dir` succeeds on a destination peer during inline directory creation, that peer's snapshot row for the created directory has its `last_seen` set to the current sync timestamp.
- `02.39` — When the group decides to delete an entry from a peer that still has it live in its listing (e.g., a deletion vote wins under rule 4/4b, or canon lacks the entry), that peer's existing snapshot row for the entry has its `deleted_time` set to the row's current `last_seen` value.

## Notes

Per-entry decision logic and the role of snapshot rows live in `03_decision-rules.md` and `03_directory-decisions.md`. Listing-error handling at a single peer is in `04_error-handling.md`.
