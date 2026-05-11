# Bounded-concurrency, keep-alive connection pool keyed by an opaque value

## Purpose
Manage one connection pool per distinct key so that concurrent callers can acquire and release connections to the same logical endpoint up to a bounded concurrency, with released connections reused while still within an idle keep-alive window. Each pool is created lazily on the first request for its key, and explicitly shut down. The component is unaware of what a "connection" is — it is parameterized by caller-supplied callbacks that open and close one. This realizes the pool semantics described in the host project's spec corpus for an `Endpoint`-keyed pool with `mc`/`ct`/`ka` settings, lazy creation, acquire/release reuse, trace events, and `close_endpoint` shutdown.

## API surface

### Settings and connections

A `PoolSettings` value carries three positive integers: `mc` (max concurrent connections held by one pool), `ct` (seconds bounding a single open attempt), `ka` (seconds an idle, released connection stays reusable before its `close` callback is invoked).

A `Connection` is whatever value the caller's `open` callback returns. The pool treats it opaquely; it neither inspects nor copies it.

### Registering a pool

`register_pool(key: opaque, open: () -> Connection, close: (Connection) -> (), settings: PoolSettings, on_event: (kind, key, in_use, mc) -> () | none) -> Pool`

The first call for a given `key` creates a pool lazily, retaining the supplied `open`/`close`/`settings`/`on_event`. Subsequent calls for the same `key` return a handle backed by the same pool; the retained callbacks and settings are not replaced. Keys are compared by value equality.

`open` returns a freshly opened `Connection`. The pool bounds each `open` invocation by `ct` seconds; on timeout or failure, `open` is treated as having failed and no slot is consumed. `close` shuts down a `Connection` value and never fails observably.

### Acquiring and releasing

`acquire(pool: Pool) -> Connection`

If the pool holds an idle `Connection` whose `ka` window has not expired, that `Connection` is removed from the idle set, its idle timer cancelled, and returned. Otherwise, if fewer than `mc` connections are currently in use, `open` is invoked (bounded by `ct`) and the returned connection is handed back. If `mc` connections are already in use and no idle connection is reusable, `acquire` blocks until capacity is freed (by a `release` or by an idle `ka` expiry) and then proceeds as above. If `open` fails, the failure is surfaced and the slot is not consumed.

`release(pool: Pool, connection: Connection)`

Returns the connection to the pool's idle set with a fresh `ka` timer. If the timer expires before another `acquire` claims the connection, `close` is invoked on it. If the pool has been shut down by the time `release` is called, the connection is closed immediately instead of pooled.

### Observation

When `on_event` is supplied, each `acquire` and `release` invokes it once with `kind` set to `"acquire"` or `"release"`, the pool's `key`, the current count of in-use connections (post-update), and the pool's `mc`. The host project uses this to emit its trace log line.

### Shutdown

`close_pool(pool: Pool)`

Marks the pool shut down, invokes `close` on every currently idle connection, and refuses subsequent `acquire` calls (an `acquire` on a shut-down pool fails). Connections that are still in use at shutdown time are not interrupted; when each is later `release`d, it is closed instead of being returned to the idle set.

## Anchoring
- Bounded-concurrency resource pool with idle keep-alive reuse: a standard concurrent-programming abstraction (a counted-resource pool / semaphore-bounded pool, as used by HTTP-client connection pools, database driver pools, and `java.util.concurrent` style bounded resource managers).
- Pool settings `mc` (max concurrent), `ct` (open timeout, seconds), `ka` (idle reuse window, seconds): named and described in the host project's SPEC for an SFTP-endpoint pool.
- Lazy per-key creation, idempotent registration, acquire/release with idle reuse, shutdown semantics for idle and in-flight connections: described in the host project's SPEC for `open_endpoint` / `acquire` / `release` / `close_endpoint`.
- Trace event on each acquire/release carrying current `in_use` and `mc`: described in the host project's SPEC under verbosity-`trace` logging of acquire/release.
- Opaque key compared by value equality, opaque `Connection` value, callback-based open/close: host-language primitives (generic / opaque types, function values).
- Blocking acquire, timers, time bounds (`ct`, `ka`): host-language primitives (concurrency primitives, monotonic time, timers).
