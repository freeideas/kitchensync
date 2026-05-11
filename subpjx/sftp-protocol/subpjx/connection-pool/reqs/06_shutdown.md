# 06_shutdown: `close_pool` closes idle connections, refuses new acquires, and defers in-use closes to release

## Behavior
`close_pool(pool)` marks the pool shut down. It invokes `close` on every currently idle connection and refuses any subsequent `acquire` (the call fails). Connections that are still in use at shutdown time are not interrupted; when each is later passed to `release`, the pool invokes `close` on it instead of returning it to the idle set. Derives from `./specs/SPEC.md` §"Shutdown".

## $REQ_IDs
- `06.1` — `close_pool` invokes `close` on every connection currently in the pool's idle set.
- `06.2` — After `close_pool`, a subsequent `acquire` on the same pool fails.
- `06.3` — Connections that are in use at the moment `close_pool` is called are not interrupted (their handles remain usable by the caller until the caller chooses to release).
- `06.4` — When a connection that was in use at shutdown is later passed to `release`, the pool invokes `close` on it and does not add it to the idle set.

## Notes
Shutdown is one-way: after `close_pool`, the pool does not re-open.
