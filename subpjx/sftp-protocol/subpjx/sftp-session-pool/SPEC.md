# SFTP Session Pool

## Purpose
Provide SSH+SFTP session establishment and pooled reuse for `sftp://` peers, including SSH authentication, host key verification, peer root-path creation, and occupancy events.

## Public API
Data shapes:

- `SftpPeer`: `user`, `password` optional, `host`, `port`, `root_path`
- `PoolSettings`: `max_open_connections` default `10`, `connection_timeout_seconds` default `30`, `idle_keep_alive_seconds` default `30`
- `PoolKey`: `user@host`
- `Session`: established SSH+SFTP session with a peer root path
- `PoolEvent`: `endpoint`, `connections`, `max`

Operations:

- `connect_listing(peer, settings) -> Session`: open an unpooled SSH+SFTP session.
- `acquire(peer, settings) -> Session`: borrow a pooled SSH+SFTP session for the peer's `PoolKey`.
- `release(session)`: return a pooled session to its pool.
- `close(session)`: close an unpooled session.
- `on_pool_event(handler)`: receive one `PoolEvent` on every pooled acquire and release.

## Behavior
Connection establishment performs an SSH handshake bounded by `connection_timeout_seconds`, authenticates using inline password first, then SSH agent, then `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, and `~/.ssh/id_rsa`. Host keys are verified through `~/.ssh/known_hosts`; unknown hosts are rejected.

After SSH+SFTP connection succeeds, the peer root path is checked. If it does not exist, it and any missing parents are created through SFTP before the session is returned.

Pooled connections are grouped by `PoolKey`. The peer root path does not affect pooling. `acquire` returns an idle session when available, otherwise opens a new session up to `max_open_connections`. If all sessions are busy at the limit, `acquire` waits until one is released. `release` returns the session to the pool and keeps it alive for up to `idle_keep_alive_seconds`; reuse resets the timer. Expired idle sessions are closed. Pools are created lazily on first successful pooled connection.

If two peers with the same `PoolKey` use different `max_open_connections` or `idle_keep_alive_seconds`, resolution is implementation-defined.

Each `PoolEvent` reports the pool key as `endpoint=<user@host>` and current occupancy as `connections=<n>/<max>`.

## Errors
Connection establishment fails if the handshake times out, authentication fails, the host key is rejected, the root path cannot be created, or an SSH/SFTP I/O failure occurs.

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

## Anchoring
`SftpPeer`, SSH handshake, host key verification, and authentication order are anchored in `sync.md` "URL Schemes" and "Authentication".

`PoolSettings`, `PoolKey`, `PoolEvent`, acquire/release semantics, idle keep-alive, connection timeout, and user+host pooling are anchored in `concurrency.md` "Connection Pool (SFTP)", "Connection Establishment", "Directory Listing", and "Trace Logging".

`Session` and SSH transport/session behavior are anchored in RFC 4253 and RFC 4254.

SFTP root-path creation is anchored in SFTP filesystem semantics from `draft-ietf-secsh-filexfer`.

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
