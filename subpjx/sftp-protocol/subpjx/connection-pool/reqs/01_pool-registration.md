# 01_pool-registration: Lazy per-key pool creation with idempotent registration

## Behavior
`register_pool(key, open, close, settings, on_event)` lazily creates one pool per distinct `key`. The first call for a given key creates the pool and retains the supplied callbacks and settings; subsequent calls for the same key return a handle to the same pool without replacing the originals. Distinct keys yield distinct pools; keys are compared by value equality. Derives from `./specs/SPEC.md` §"Registering a pool".

## $REQ_IDs
- `01.1` — A subsequent `register_pool` call with the same key returns a handle backed by the same pool (acquires from either handle draw from the same idle set and share the same `mc` capacity).
- `01.2` — The `open`/`close`/`settings`/`on_event` supplied on subsequent same-key calls do not replace the originals retained on first registration.
- `01.3` — Calls with two distinct keys produce two independent pools (capacity and idle sets are not shared between them).
- `01.4` — Two keys that compare equal by value resolve to the same pool.

## Notes
`PoolSettings` carries three positive integers `mc`, `ct`, `ka`. Their effects are observed in tiers 02–06; this concern is only about registration mechanics.
