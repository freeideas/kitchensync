# 03_per-key-isolation: Per-key independence and factory exceptions

## Behavior
The per-key cap applies independently to each key — the pool imposes no global cap — and `create` is invoked outside the pool's internal locks so that a slow `create` for one key does not stall acquirers of other keys. If `create` raises, the exception reaches the calling acquirer and no slot is consumed for that key. Derived from `specs/SPEC.md`, sections "API surface" (per-key independence, factory outside locks) and the surrounding paragraph on `create` exception behavior.

## $REQ_IDs
- `03.1` — `max_per_key` is enforced independently per key: reaching the cap for one key does not prevent acquirers of a different key from obtaining resources up to that key's own cap.
- `03.2` — A slow `create` for one key does not delay or block `acquire` calls for other keys.
- `03.3` — When `create` raises, the exception propagates to the calling acquirer.
- `03.4` — When `create` raises, no slot is consumed for that key: a subsequent `acquire` for the same key can proceed without being blocked by the failed creation.
