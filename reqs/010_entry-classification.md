# 010_entry-classification: Per-peer entry classification

## Behavior
This concern derives from `specs/multi-tree-sync.md` section "Entry
Classification" and the classification half of "Decision Rules" timestamp
tolerance.

It covers how each contributing peer's live state for a file entry is classified
against that peer's own snapshot row, producing one of: unchanged (live with
matching mod_time and byte_size, row present, not deleted), modified (live but
mod_time or byte_size differs, including resurrection where `deleted_time` was
set and is cleared), new (live with no row), deleted (absent with a tombstoned
row), absent-unconfirmed (absent with a row whose `deleted_time` is NULL), and
no-opinion (absent with no row). It covers that a file is "unchanged" only when
both mod_time and byte_size match, and that the 5-second timestamp tolerance
applies when comparing a peer's live mod_time to its snapshot row's mod_time.

How classified states are combined to pick a winner is `011_decision-rules`.
Directory state (which is existence-based, not classified by mod_time) is
`012_directory-and-type-decisions`.

## $REQ_IDs

- `010.1` -- A live file whose byte_size equals its snapshot row's byte_size and whose mod_time is within 5 seconds of the row's mod_time, with the row's `deleted_time` NULL, is treated as unchanged and is not re-copied between peers that already match.
- `010.2` -- A live file whose byte_size differs from its snapshot row's byte_size is treated as modified even when its mod_time matches the row.
- `010.3` -- A live file whose mod_time differs from its snapshot row's mod_time by more than 5 seconds is treated as modified even when its byte_size matches the row.
- `010.4` -- A live file present where its snapshot row has a non-NULL `deleted_time` is treated as a modification (resurrection): the live file is propagated and the tombstone does not cause the entry to be deleted.
- `010.5` -- A live file with no snapshot row on a peer is treated as new on that peer and is propagated to peers that lack it.
- `010.6` -- An absent entry whose snapshot row has a non-NULL `deleted_time` is classified as deleted on that peer, with `deleted_time` as its deletion estimate.
- `010.7` -- An absent entry whose snapshot row has a NULL `deleted_time` is classified as absent-unconfirmed on that peer, not as a recorded deletion.
- `010.8` -- An absent entry with no snapshot row on a peer produces no opinion from that peer, so that peer alone does not cause the entry to be removed from peers that have it.

## Notes

- Classification is an intermediate per-peer judgment; its only external proof
  runs through the decision outcomes owned by `011_decision-rules` and the
  snapshot writes owned by `017_snapshot-updates`. These bullets isolate a single
  peer's classification and assert the spec's classification mapping; combining
  classifications into a winner stays in `011`.
- Resurrection's clearing of `deleted_time` is the confirmed-present snapshot
  write owned by `017_snapshot-updates`; `010.4` asserts only that a live file
  over a tombstoned row is classified as a modification rather than a deletion.
