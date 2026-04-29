# 03_decision-rules: Newest-wins, deletion vs existing, ties, timestamp tolerance

## Behavior

Without a canon peer, decisions on each entry are made by comparing peer state against per-peer snapshot rows. Newest mod_time wins, deletions compete with newer modifications, and ties keep data. Comparisons use a 5-second timestamp tolerance throughout. Derived from `./specs/multi-tree-sync.md` (`Entry Classification`, `Decision Rules` 1–6, tolerance paragraph).

## $REQ_IDs
- `03.21` — When two peers' files have the same mod_time and byte_size, no copy is performed and both are considered unchanged.
- `03.22` — When two peers hold the same file but with different mod_times beyond the 5-second tolerance, the one with the newer mod_time overwrites the older.
- `03.23` — When one peer's mod_time is within 5 seconds of the other peer's, neither is treated as newer (no overwrite is forced solely on that difference).
- `03.24` — A new file present on one contributing peer and absent without history on another is propagated to peers that lack it.
- `03.25` — When one peer has deleted a file (snapshot tombstone) and another peer still has it, with `deleted_time` later than the file's mod_time, the deletion wins and the file is displaced to BAK/ on the peers that have it.
- `03.26` — When one peer has deleted a file but another peer's mod_time is at or after the deletion estimate (within tolerance), the existing file wins and is propagated to peers that lack it.
- `03.27` — When a peer is absent for an entry whose snapshot row has `deleted_time = NULL` and `last_seen` is NULL or not more than 5 seconds beyond any other peer's mod_time for that entry, no deletion is inferred — the entry is re-copied to that peer instead.
- `03.28` — When two peers have the same mod_time but different `byte_size`, the larger file wins.
- `03.29` — In existence-vs-deletion ties, existence wins (data is kept, not deleted).
- `03.30` — A peer that has never had a snapshot row for an entry does not vote on it; once a winner is decided, it receives a copy if the winner is "exists".
- `03.31` — When multiple peers have a tombstone for the same entry and another peer still has the file, the deletion estimate compared against that file's mod_time is the most recent `deleted_time` among the deleting peers.
