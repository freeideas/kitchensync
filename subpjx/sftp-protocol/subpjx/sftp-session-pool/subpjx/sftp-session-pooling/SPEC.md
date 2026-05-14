# SFTP Session Pooling

## Purpose
Provide pooled reuse of established SSH+SFTP sessions for `sftp://` peers, including per-`PoolKey` connection limits, idle keep-alive, and occupancy events.

## Public API
Data shapes:

- `SftpPeer`: `user`, `password` optional, `host`, `port`, `root_path`
- `PoolSettings`: `max_open_connections` default `10`, `idle_keep_alive_seconds` default `30`
- `PoolKey`: `user@host`
- `Session`: established SSH+SFTP session with a peer root path
- `PoolEvent`: `endpoint`, `connections`, `max`

Operations:

- `acquire(peer, settings) -> Session`: borrow a pooled SSH+SFTP session for the peer's `PoolKey`.
- `release(session)`: return a pooled session to its pool.
- `on_pool_event(handler)`: receive one `PoolEvent` on every pooled acquire and release.

## Behavior
Pooled connections are grouped by `PoolKey`. The peer root path does not affect pooling.

`acquire` returns an idle session when available, otherwise opens a new session up to `max_open_connections`. If all sessions are busy at the limit, `acquire` waits until one is released.

`release` returns the session to the pool and keeps it alive for up to `idle_keep_alive_seconds`; reuse resets the timer. Expired idle sessions are closed. Pools are created lazily on first successful pooled connection.

If two peers with the same `PoolKey` use different `max_open_connections` or `idle_keep_alive_seconds`, resolution is implementation-defined.

Each `PoolEvent` reports the pool key as `endpoint=<user@host>` and current occupancy as `connections=<n>/<max>`.

## Errors
`acquire` fails if opening a new SSH+SFTP session fails.

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

## Anchoring
`SftpPeer` is anchored in `sync.md` "URL Schemes".

`PoolSettings`, `PoolKey`, acquire/release semantics, idle keep-alive, and user+host pooling are anchored in `concurrency.md` "Connection Pool (SFTP)" and "Connection Establishment".

`PoolEvent` and occupancy reporting are anchored in `concurrency.md` "Trace Logging".

`Session` and SSH transport/session behavior are anchored in RFC 4253 and RFC 4254.

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
