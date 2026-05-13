# 01_pool-lifecycle: Pool construction and shutdown

## Behavior
A pool is constructed with caller-supplied `create` and `destroy` callables and configured with a per-key concurrency cap and an idle keep-alive TTL. `shutdown()` ends the pool's lifetime: every live resource is torn down, further operations are refused, and any acquirer currently blocked is released with a shutdown error. Derived from `specs/SPEC.md`, sections "API surface" (construction parameters, `shutdown` operation).

## $REQ_IDs
- `01.1` — `shutdown()` invokes `destroy` on every live resource for every key, covering both idle and held resources.
- `01.2` — After `shutdown()`, subsequent operations on the pool are refused.
- `01.3` — Acquirers blocked on `acquire` at the time `shutdown()` is called are released with a shutdown error rather than a resource.

## Notes
Handle validity outside the `acquire`-to-`release`/`discard`/`shutdown` window is described in the spec as a programming error with no defined behavior, so it is not enumerated as a requirement.
