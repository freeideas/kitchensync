# 03_shutdown: Endpoint shutdown closes the pool and refuses further acquires

## Behavior

`close_endpoint(endpoint)` shuts the endpoint's pool down: every idle connection is closed and subsequent `acquire` calls on that endpoint are refused. Operations on a `Connection` that is still in flight (not yet released) are allowed to complete, after which that connection is closed rather than returned to the pool. Derived from `SPEC.md` §"Shutdown".

## $REQ_IDs
- `03.20` — `close_endpoint(endpoint)` closes every idle connection in that endpoint's pool.
- `03.21` — After `close_endpoint(endpoint)` returns, subsequent `acquire(endpoint)` calls are refused.
- `03.22` — An in-flight operation on a `Connection` that has not yet been released when `close_endpoint(endpoint)` is invoked is allowed to complete.
- `03.23` — A `Connection` whose in-flight operation completed after `close_endpoint(endpoint)` is closed instead of being returned to the pool.
