# 02_decision-rules: Per-entry decision rules for files

## Behavior

For each file entry at a directory level, the winning state is chosen from contributing peers' live states and per-peer snapshot rows. Newest mod_time wins for live files; deletion vs existing is decided by comparing the deletion estimate against the live mod_time; ties keep data. A 5-second tolerance applies to all timestamp comparisons. Derived from `specs/multi-tree-sync.md` §"Entry Classification" and §"Decision Rules".

## $REQ_IDs
- `02.19` — When every contributing peer has the entry unchanged (live mod_time matches its snapshot row within 5s), no copy is enqueued.
- `02.20` — When contributing peers' live mod_times differ, the entry with the newest mod_time wins and is pushed to all peers that don't match (within the 5-second tie tolerance).
- `02.21` — An entry that is new on one peer (no snapshot row there) and absent on others is propagated to all peers that lack it.
- `02.22` — When one peer's snapshot shows the entry deleted and another peer has it live: if the deletion estimate is later than the live mod_time by more than 5 seconds, the deletion wins and the file is displaced on peers that have it; otherwise the live file wins and is pushed to peers that lack it.
- `02.23` — When multiple peers have deleted the entry, the most recent deletion estimate among them is used as the deletion estimate.
- `02.24` — An absent-unconfirmed entry (peer is absent, snapshot row exists with `deleted_time` NULL) counts as a deletion using `last_seen` as the estimate only if `last_seen` exceeds the max live mod_time among peers that have it by more than 5 seconds; otherwise it triggers a re-copy with no deletion vote.
- `02.25` — When live mod_times are equal (within 5 seconds) but byte sizes differ, the larger file wins.
- `02.26` — If no contributing peer has the entry at all, no copy is enqueued and any subordinate peer that has the entry has it displaced to BAK/.
- `02.27` — A peer that already has a matching mod_time (within 5s) and matching byte size to the winning entry is not copied to.
- `02.28` — A peer that has the entry live but whose snapshot row is a tombstone (`deleted_time` NOT NULL) is treated as a modified (resurrected) entry — its mod_time participates in the decision as a live vote.

## Notes
Directory existence-based decisions and type conflicts are in `03_directory-decisions.md`. Canon overrides apply across all of these — see `03_canon-peer.md`. Subordinate peers do not contribute votes — see `03_subordinate-peer.md`.
