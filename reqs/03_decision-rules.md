# 03_decision-rules: File-entry decision logic

## Behavior

For each file entry at a directory level, the program compares peer states to their snapshot rows to classify the entry, then applies newest-wins and conservation-of-data rules to pick a winning state. Decisions consider only contributing (non-subordinate) peers. Derived from `multi-tree-sync.md` §"Entry Classification" and §"Decision Rules".

## $REQ_IDs

- `03.1` — When all contributing peers agree on an existing file (same mod_time, same size), no copy is enqueued.
- `03.2` — When contributing peers disagree on a file's mod_time, the file with the newest mod_time is propagated to peers whose copy is older.
- `03.3` — When a file is new on one contributing peer and absent on others that have no snapshot row for it, the file is copied to those other peers.
- `03.4` — When a file exists on one contributing peer and is deleted on another (snapshot row with `deleted_time` set), and the deletion estimate is more than 5 seconds after the existing file's mod_time, the file is displaced on all peers that have it.
- `03.14` — When a file exists on one contributing peer and is deleted on another (snapshot row with `deleted_time` set), and the deletion estimate is not more than 5 seconds after the existing file's mod_time, the existing file is propagated to peers whose snapshot row has the tombstone.
- `03.5` — When a peer's snapshot row says it had a file (no `deleted_time`) but the file is now absent, and `last_seen` is NULL or does not exceed the max mod_time of peers that still have the entry by more than 5 seconds, the file is re-copied to that peer.
- `03.18` — When a peer's snapshot row says it had a file (no `deleted_time`) but the file is now absent, and `last_seen` exceeds the max mod_time of peers that still have the entry by more than 5 seconds, the file is displaced on all peers that have it.
- `03.6` — When peers' mod_times match (within ±5 seconds) but byte sizes differ, the larger file wins.
- `03.7` — Mod_time comparisons throughout decision-making use a ±5-second tolerance: peers within that window of the maximum are tied with it.
- `03.8` — When no contributing peer has and has ever had the entry, no copy is enqueued for that entry.
- `03.85` — When multiple contributing peers have deleted the entry, the most recent deletion estimate is used as the deletion time in the comparison against a surviving file's mod_time.
- `03.91` — When a contributing peer's snapshot row has `deleted_time` set but the entry is now live in that peer's listing (resurrection), the entry is classified as modified for decision purposes.
- `03.19` — On a resurrection (live entry whose snapshot row had `deleted_time` set), the peer's snapshot row has `deleted_time` cleared back to NULL during this run's snapshot update.
- `03.92` — When a destination peer already has the winning file (mod_time within ±5 seconds and matching byte_size), no file copy is performed for that destination — only the peer's snapshot row is created or updated.
- `03.110` — A contributing peer with no snapshot row for a file does not vote on the winner but, when the decided state is an existing file, receives that file if it lacks it.

## Notes

Subordinate-peer states never appear in `gather_states` — they receive outcomes but do not vote. Canon-peer overrides bypass these rules and are covered in `03_canon-peer.md`.
