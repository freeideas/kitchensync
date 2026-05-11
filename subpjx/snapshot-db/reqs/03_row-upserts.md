# 03_row-upserts: Write rows for entries observed or decided on the peer, plus completion confirmation.

## Behavior
Snapshot rows are written through three operations. The confirmed-present upsert records that an entry was observed on the peer (or that a copy completed there) — it sets `last_seen` to the supplied timestamp and clears any prior `deleted_time`. The decided-but-unconfirmed upsert records a planned state before the copy completes — it writes the supplied `mod_time` and `byte_size` and clears `deleted_time`, but deliberately leaves `last_seen` untouched (preserving any prior value, or NULL if no prior row). The mark-copy-completed operation later stamps `last_seen` once the copy finishes. Each operation writes the row keyed by the path's identifier so it can be looked up afterward. Derived from `SPEC.md` §"Row operations".

## $REQ_IDs
- `03.1` — A confirmed-present upsert for `(path, basename, mod_time, byte_size, ts)` produces a row that can be looked up by `hash(path)` and whose `basename`, `mod_time`, and `byte_size` equal the supplied values.
- `03.2` — A confirmed-present upsert sets the row's `last_seen` to the supplied confirmation timestamp.
- `03.3` — A confirmed-present upsert clears `deleted_time` (sets it to NULL), even if the prior row was a tombstone.
- `03.4` — A confirmed-present upsert on a path with an existing row replaces the stored `mod_time` and `byte_size` with the supplied values.
- `03.5` — A decided-but-unconfirmed upsert for `(path, basename, mod_time, byte_size)` produces a row that can be looked up by `hash(path)` and whose `basename`, `mod_time`, and `byte_size` equal the supplied values.
- `03.6` — A decided-but-unconfirmed upsert clears any prior `deleted_time`.
- `03.7` — Mark-copy-completed for `(path, ts)` sets `last_seen` to `ts` on the existing row for that path.
- `03.15` — A decided-but-unconfirmed upsert leaves `last_seen` equal to the prior row's `last_seen`, or NULL if no prior row existed.

## Notes
The two upsert variants exist because the orchestrator records the decision before the copy and stamps confirmation afterward; preserving prior `last_seen` during the decided phase is what makes the timing distinction observable to later passes. Path-id derivation is verified indirectly via lookup-by-id after the upsert.
