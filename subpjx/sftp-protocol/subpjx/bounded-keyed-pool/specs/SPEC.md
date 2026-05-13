# bounded-keyed-pool

A bounded keyed resource pool with idle keep-alive.

## Purpose
A generic concurrency primitive that manages reusable resources grouped by key. For each key, the pool maintains up to a fixed maximum of concurrent live resources, blocking acquirers when the cap is reached and reusing released resources whenever possible. Released resources remain warm for a configurable idle window before being torn down.

## API surface

The pool is constructed with:

- a factory `create(key) -> resource` that produces a new resource for a key,
- a destructor `destroy(resource)` that tears a resource down,
- `max_per_key` (integer ≥ 1): the maximum number of concurrent live resources permitted for any one key,
- `idle_ttl_seconds` (number ≥ 0): how long a released resource remains warm before destruction.

Operations:

- `acquire(key) -> handle`. If an idle resource is cached for `key`, returns it (now held, no longer idle). Otherwise, if fewer than `max_per_key` resources currently exist for that key (idle + held combined), calls `create(key)` and returns a fresh handle wrapping the result. Otherwise blocks until a held resource for `key` is released or discarded, then acquires as above. Blocking is unbounded; callers wanting a timeout enforce it externally. Acquirers waiting on the same key are served in FIFO order.
- `release(handle)`: returns the resource to the idle cache for its key. The idle timer is started at `idle_ttl_seconds`; reuse via a subsequent `acquire` before the timer fires resets it. If the timer expires without reuse, `destroy` is called on the resource and the slot is freed.
- `discard(handle)`: drops the resource without returning it to the idle cache. `destroy` is called immediately and the slot is freed. Callers use this when a resource is known to be unusable.
- `shutdown()`: destroys every live resource (idle and held) and refuses subsequent operations. Acquirers blocked on `acquire` at the time of shutdown are released with a shutdown error.

`max_per_key` applies independently to each key; the pool imposes no global cap on total resources across all keys.

The factory `create` is invoked outside the pool's internal locks so that a slow `create` for one key does not stall acquirers of other keys. If `create` raises, no slot is consumed for that key and the exception propagates to the calling acquirer.

A handle is valid only until passed to `release` or `discard` (or until `shutdown` is called); using a handle outside that window is a programming error.

## Anchoring

- `key`: an opaque caller-supplied value used only for equality and hashing — host-language primitive.
- `resource`, `handle`: opaque values; `resource` is whatever the factory returns, `handle` is whatever the pool issues from `acquire` and accepts on `release` / `discard` — host-language primitive.
- `create`, `destroy`: caller-supplied callables — host-language primitive.
- Bounded resource pool with idle keep-alive: a standard concurrency abstraction (e.g., JDBC connection pools, HTTP keep-alive pools, generic object-pool libraries).
- Blocking acquire on a per-key cap with FIFO queueing: standard producer-consumer pattern — host-language concurrency primitives (semaphore / condition variable).
- Idle timer expiry: scheduled future / timer — host-language primitive.
