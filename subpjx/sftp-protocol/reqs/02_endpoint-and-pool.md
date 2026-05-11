# 02_endpoint-and-pool: Endpoint handles and per-(user,host) connection pools

## Behavior

The transport exposes endpoints, which are pool keys identified by SFTP user and host. `open_endpoint(user, host, port, password, settings)` returns an endpoint handle and lazily creates a per-(user, host) connection pool on the first call. Subsequent `open_endpoint` calls for the same (user, host) — regardless of supplied port (default vs `22`) — return a handle backed by the same pool. `PoolSettings` carries the pool's `mc` (max concurrent), `ct` (connection timeout seconds), and `ka` (idle keep-alive seconds). Derived from `SPEC.md` §"Endpoints and pools".

## $REQ_IDs
- `02.1` — `open_endpoint(user, host, port, password, settings)` returns an endpoint handle that operations can be performed against.
- `02.2` — Two `open_endpoint` calls with the same `user` and `host` return handles backed by the same connection pool, irrespective of whether one supplies port `22` and the other supplies the default port.
- `02.3` — Two `open_endpoint` calls with different `user` or different `host` return handles backed by distinct connection pools.

## Notes

Conflicting `mc`/`ka` values across `open_endpoint` calls for the same (user, host) are explicitly implementation-defined in the spec, so no bullet pins that behavior.
