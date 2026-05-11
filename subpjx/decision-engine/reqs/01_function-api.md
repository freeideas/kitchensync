# 01_function-api: pure callable API with two operations and shared configuration

## Behavior
The component exposes two operations to host code: `decide(entry_name, per_peer_inputs) → decision` and `classify_file(listing_state, snapshot_row) → classification`. Both operations share configuration supplied once at construction time: `timestamp_tolerance_seconds` (the tolerance window used by classification and rule comparisons) and `now` (the timestamp the operation assigns to any `last_seen` or `deleted_time` directives it emits). The component is a deterministic pure function: same inputs yield the same outputs, with no filesystem I/O, no networking, no SQL, and no internal clock reads. Derived from `./specs/SPEC.md` §"API surface" (operation signatures, configuration paragraph) and §"Anchoring" determinism entry.

## $REQ_IDs
- `01.1` — Every `last_seen` or `deleted_time` value emitted in the decision matches the configured `now` (the operation does not read the system clock).
- `01.2` — Calling `decide` twice with identical inputs and identical construction-time configuration returns equivalent decision records.
- `01.3` — Calling `classify_file` twice with identical inputs and identical configuration returns the same classification.
- `01.4` — Invoking `decide` or `classify_file` performs no observable filesystem writes, no network access, and no database queries.
- `01.5` — Each per-peer entry in the returned decision is identified by the `peer_id` from its corresponding input record, allowing the caller to correlate per-peer actions and directives back to its input handles.

## Notes
`peer_id` is opaque to the engine — it appears unchanged on the per-peer output entries so the caller can correlate decisions back to its own transport handles.
