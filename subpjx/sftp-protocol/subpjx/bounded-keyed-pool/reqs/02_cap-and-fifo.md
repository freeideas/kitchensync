# 02_cap-and-fifo: Per-key concurrency cap with FIFO blocking

## Behavior
The pool enforces a per-key cap on concurrent live resources (`max_per_key`, counting idle + held combined). While the cap has room, `acquire` proceeds immediately; once the cap is reached and all resources are held, additional acquirers block until a resource for that key is released or discarded. Multiple acquirers waiting on the same key are served in arrival order. Derived from `specs/SPEC.md`, section "API surface" (`acquire` blocking semantics, FIFO ordering).

## $REQ_IDs
- `02.1` — When fewer than `max_per_key` resources exist for a key (idle + held combined), `acquire(key)` returns without blocking.
- `02.2` — When `max_per_key` resources exist for a key and all of them are held, `acquire(key)` blocks until a resource for that key is released or discarded.
- `02.3` — When a held resource for a key is released or discarded, a blocked acquirer for that key proceeds and receives a resource.
- `02.4` — Multiple acquirers blocked on the same key are served in FIFO (arrival) order as resources become available.
