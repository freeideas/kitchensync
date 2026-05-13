# 00_artifacts: Library exposes a pure decision operation

## Behavior
The library exposes `decide_entry` as the primary operation for deciding one entry from participant roles, observations, history records, and tolerance. The operation is pure and performs no filesystem, networking, storage, or other I/O. Derived from SPEC.md "Purpose" and "API surface" → "Inputs" and "Output".

## $REQ_IDs
- `00.1` — Calling `decide_entry` with participant roles, observations, history records, and tolerance returns one decision for the entry.
- `00.2` — Calling `decide_entry` with the same inputs returns the same decision.
- `00.3` — Calling `decide_entry` performs no filesystem, networking, storage, or other I/O.
