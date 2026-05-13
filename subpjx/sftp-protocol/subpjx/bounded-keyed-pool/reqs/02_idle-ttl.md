# 02_idle-ttl: Idle keep-alive timer

## Behavior
A released resource is kept warm in the idle cache for up to `idle_ttl_seconds`. If it is reused via `acquire` before the timer fires, destruction is averted; if the timer expires without reuse, the pool calls `destroy` on the resource and frees its slot for new resources on that key. Derived from `specs/SPEC.md`, section "API surface" (`release` and idle-timer semantics).

## $REQ_IDs
- `02.1` — A released resource that is not reused within `idle_ttl_seconds` has `destroy` invoked on it.
- `02.2` — After idle-TTL expiry destroys a resource, its slot is freed: a fresh `acquire` for that key can create a new resource without being blocked by the expired one.
- `02.3` — Reusing an idle resource via `acquire` before its idle timer fires prevents `destroy` from being invoked for that release.
