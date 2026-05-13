# 02_pool-acquire-release: Pool acquire, release, idle keep-alive, and shutdown

## Behavior

Callers obtain a connection handle from the pool via `acquire(url)`, do work, then call `release(handle)` to return it. The pool reuses idle sessions when possible, opens new sessions up to `max_connections`, and blocks acquirers when the cap is reached. Released sessions stay warm for `idle_keepalive_seconds`; if not reused within that window the underlying SSH+SFTP session is torn down. A single connection attempt's SSH handshake is bounded by `connect_timeout_seconds`. `shutdown()` closes every cached and in-use session. Derives from `specs/SPEC.md` § "API surface > Pool".

## $REQ_IDs

- `02.2` — `acquire(url)` opens a new SSH+SFTP session when no cached session is available and the pool is below `max_connections`.
- `02.3` — When `max_connections` sessions are open and all are busy, a further `acquire` blocks until another handle is released.
- `02.5` — A released session that is reused before `idle_keepalive_seconds` elapses serves the next `acquire` for the same `(user, host)`.
- `02.6` — A released session that is not reused within `idle_keepalive_seconds` has its underlying SSH+SFTP session closed.
- `02.7` — Reusing a released session before its idle timer expires resets the timer.
- `02.8` — `shutdown()` closes all sessions in the pool (cached and in-use).
- `02.9` — `max_connections` defaults to 10 when not overridden.
- `02.42` — `connect_timeout_seconds` defaults to 30 when not overridden.
- `02.43` — `idle_keepalive_seconds` defaults to 30 when not overridden.

## Notes

- The handshake-timeout error itself is surfaced as an I/O error — see [[03_error-categories]].
