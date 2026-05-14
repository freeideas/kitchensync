# SFTP Protocol

## Purpose
Provide SSH/SFTP filesystem access for `sftp://` peers, including SSH authentication, SFTP file operations, root-path creation, and pooled SSH+SFTP connections keyed by user+host.

## Public API
Data shapes:

- `SftpPeer`: `user`, `password` optional, `host`, `port`, `root_path`
- `PoolSettings`: `max_open_connections` default `10`, `connection_timeout_seconds` default `30`, `idle_keep_alive_seconds` default `30`
- `PoolKey`: `user@host`
- `Entry`: `name`, `is_dir`, `mod_time`, `byte_size`
- `Stat`: `is_dir`, `mod_time`, `byte_size`
- `ReadHandle`
- `WriteHandle`
- `PoolEvent`: `endpoint`, `connections`, `max`

Operations:

- `connect_listing(peer, settings) -> Session`: open an unpooled SSH+SFTP session for directory listing.
- `acquire(peer, settings) -> Session`: borrow a pooled SSH+SFTP session for the peer's `PoolKey`.
- `release(session)`: return a pooled session to its pool.
- `close(session)`: close an unpooled session.
- `on_pool_event(handler)`: receive one `PoolEvent` on every pooled acquire and release.
- `list_dir(session, path) -> Entry[]`
- `stat(session, path) -> Stat`
- `open_read(session, path) -> ReadHandle`
- `read(session, handle, max_bytes) -> bytes | EOF`
- `close_read(session, handle)`
- `open_write(session, path) -> WriteHandle`
- `write(session, handle, bytes)`
- `close_write(session, handle)`
- `rename(session, src, dst)`
- `delete_file(session, path)`
- `create_dir(session, path)`
- `delete_dir(session, path)`
- `set_mod_time(session, path, time)`

All operation paths are relative to the peer root path.

## Behavior
Connection establishment performs an SSH handshake bounded by `connection_timeout_seconds`, authenticates using inline password first, then SSH agent, then `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, and `~/.ssh/id_rsa`. Host keys are verified through `~/.ssh/known_hosts`; unknown hosts are rejected.

After SSH+SFTP connection succeeds, the peer root path is checked. If it does not exist, it and any missing parents are created through SFTP before the session is returned.

Pooled connections are grouped by `PoolKey`. The peer root path does not affect pooling. `acquire` returns an idle session when available, otherwise opens a new session up to `max_open_connections`. If all sessions are busy at the limit, `acquire` waits until one is released. `release` returns the session to the pool and keeps it alive for up to `idle_keep_alive_seconds`; reuse resets the timer. Expired idle sessions are closed. Pools are created lazily on first successful pooled connection.

If two peers with the same `PoolKey` use different `max_open_connections` or `idle_keep_alive_seconds`, resolution is implementation-defined.

`list_dir` returns only immediate regular-file and directory children. Symbolic links, devices, FIFOs, sockets, and other special entries are omitted. `stat` reports symbolic links and special entries as not found.

`open_write` creates the target file and missing parent directories as needed. `rename` is a same-filesystem rename. `delete_dir` removes an empty directory. `set_mod_time` sets the modification time supplied by the caller.

Each `PoolEvent` reports the pool key as `endpoint=<user@host>` and current occupancy as `connections=<n>/<max>`.

## Errors
Connection establishment fails if the handshake times out, authentication fails, the host key is rejected, the root path cannot be created, or an SSH/SFTP I/O failure occurs.

Filesystem operations return only these categories:

- `not_found`
- `permission_denied`
- `io_error`

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

`stat` on a symbolic link or special file returns `not_found`.

If a write, close, rename, delete, directory creation, or mod-time update cannot complete because of permissions, it returns `permission_denied`. Other transport or filesystem failures return `io_error`.

## Anchoring
`SftpPeer`, SSH handshake, host key verification, and authentication order are anchored in `sync.md` "URL Schemes" and "Authentication".

`list_dir`, `stat`, streaming handles, `rename`, `delete_file`, `create_dir`, `delete_dir`, and `set_mod_time` are anchored in `sync.md` "Peer Transports".

`Entry`, `Stat`, `mod_time`, `byte_size`, regular-file filtering, symbolic-link handling, and special-file handling are anchored in `sync.md` "Peer Transports" and `ignore.md` "Symlinks" / "Built-in Excludes".

`PoolSettings`, `PoolKey`, `PoolEvent`, acquire/release semantics, idle keep-alive, connection timeout, and user+host pooling are anchored in `concurrency.md` "Connection Pool (SFTP)", "Connection Establishment", "Directory Listing", and "Trace Logging".

SFTP filesystem semantics are anchored in `draft-ietf-secsh-filexfer`. SSH transport/session behavior is anchored in RFC 4253 and RFC 4254.

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
