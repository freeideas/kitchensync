# 02_connection-lifecycle: Acquire, release, and pool idle-reuse semantics

## Behavior

`acquire(endpoint)` returns an open `Connection` to the endpoint, blocking when the pool's `mc` cap is saturated. A previously released connection still within its `ka` idle window is reused; otherwise a new SSH+SFTP session is established with the SSH handshake bounded by `ct` seconds. Handshake or authentication failures surface as I/O errors. `release(connection)` returns the connection to its pool; the connection remains idle for up to `ka` seconds — reuse within that window resets the idle timer, otherwise the underlying SSH+SFTP session is closed. Derived from `SPEC.md` §"Acquiring and releasing pooled connections".

## $REQ_IDs
- `02.10` — `acquire(endpoint)` returns an open `Connection` to the endpoint.
- `02.11` — When `mc` connections are already in use and all are busy, a further `acquire` blocks until one is released, and resumes once a connection becomes available.
- `02.12` — A connection released within the last `ka` seconds is reused on the next `acquire` rather than establishing a fresh SSH+SFTP session.
- `02.13` — A fresh-connection SSH handshake is bounded by `ct` seconds; expiry surfaces an I/O error.
- `02.14` — A handshake or authentication failure on a fresh connection is surfaced as an I/O error.
- `02.15` — `release(connection)` returns the connection to its pool, freeing a slot for a blocked or subsequent `acquire`.
- `02.16` — Reusing a released connection within its `ka` window resets the keep-alive timer (the connection then remains idle-eligible for another `ka` seconds after the most recent release).
- `02.17` — A released connection that goes unused for more than `ka` seconds has its underlying SSH+SFTP session closed.
