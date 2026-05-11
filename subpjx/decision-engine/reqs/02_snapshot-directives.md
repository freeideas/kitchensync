# 02_snapshot-directives: per-peer snapshot update directives accompany every action

## Behavior
Each per-peer action in a decision is paired with a snapshot update directive that the caller persists. The directive describes how to mutate the peer's snapshot row: confirm a live observation, record a decided target that has not yet been copied, mark a tombstone, clear a tombstone, or leave the row alone. The `set_last_seen` flag on `upsert_present` distinguishes "we observed this peer's entry live in this run" from "we decided this peer should hold this content but haven't copied it yet." Derived from `./specs/SPEC.md` §"API surface" snapshot-update paragraph and §"Anchoring" Snapshot Updates entry.

## $REQ_IDs
- `02.25` — Every per-peer entry in the decision carries one snapshot update directive co-located with its action.
- `02.26` — Each snapshot directive is one of: `upsert_present`, `upsert_decided_target`, `mark_tombstone`, `clear_tombstone`, `no_change`.
- `02.27` — `upsert_present` carries `mod_time`, `byte_size`, and a `set_last_seen` flag indicating the listing was observed live this run.
- `02.28` — `upsert_decided_target` carries `mod_time` and `byte_size` for the decided target and leaves `last_seen` unchanged (the data has not been copied yet).
- `02.29` — `mark_tombstone` carries a `deleted_time` equal to the configured `now`.
- `02.30` — A peer whose listing confirms its prior snapshot row live (still present, content matches) receives `upsert_present` with `set_last_seen` true.
- `02.31` — A peer whose listing reports the entry `absent` and whose prior snapshot row was live receives `mark_tombstone`.
- `02.32` — A peer whose listing is live but whose snapshot row is tombstoned (resurrection) receives `clear_tombstone`.
