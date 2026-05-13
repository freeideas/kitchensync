# 02_acquire-and-release: Acquire, release, discard, and idle reuse

## Behavior
The pool's central operation set: `acquire(key)` produces a handle (either freshly created or reused from the idle cache), `release(handle)` returns the resource to the idle cache where it remains available for reuse on the same key, and `discard(handle)` tears the resource down immediately and frees its slot. Derived from `specs/SPEC.md`, section "API surface" (`acquire`, `release`, `discard`).

## $REQ_IDs
- `02.1` — When no idle resource is cached for `key`, `acquire(key)` invokes `create(key)` and returns a handle wrapping the newly created resource.
- `02.2` — When an idle resource is cached for `key`, `acquire(key)` returns that cached resource and does not invoke `create`.
- `02.3` — `discard(handle)` invokes `destroy` on the resource immediately.
- `02.4` — After `discard(handle)`, a subsequent `acquire(key)` for the same key proceeds without blocking on the discarded resource.
