# SFTP transport over SSH with per-endpoint connection pool

## Purpose
Provide kitchensync's `sftp://` transport: the SSH+SFTP wire protocol plus the connection pool keyed by user+host. Other peers (via the host language's standard `file://` stdlib) and the SFTP transport implemented here expose the same operation surface so the caller's sync logic does not branch on transport. This component implements `sync.md` §"Peer Transports" (operation surface), `sync.md` §"Authentication" (key search chain and known_hosts verification), and `concurrency.md` §"Connection Pool (SFTP)" (per-endpoint pool semantics, `mc`/`ct`/`ka` settings, lazy pool creation, trace logging of acquire/release).

The component has no knowledge of the rest of kitchensync — it is told an endpoint and the per-pool settings, and it returns a transport object that performs operations against the remote filesystem.

## API surface

### Endpoints and pools

`Endpoint` is the pool key: an SFTP user and host (with optional non-default port). Two endpoints with the same user and host (ignoring port 22 vs default) refer to the same remote account and share a pool.

`open_endpoint(user: string, host: string, port: int | default, password: string | none, settings: PoolSettings) -> Endpoint`

Returns an endpoint handle. The first call for a given (user, host) creates its pool lazily; subsequent calls for the same (user, host) return a handle backed by the same pool. `PoolSettings` carries `mc` (max concurrent connections, positive int), `ct` (connection timeout seconds, positive int), and `ka` (idle keep-alive TTL seconds, positive int). Per-URL `PoolSettings` from the caller override the per-pool defaults at acquire time as documented in `concurrency.md`.

If two distinct calls for the same (user, host) supply conflicting `mc` or `ka` values, the resolution is implementation-defined (this is a misuse, per `concurrency.md`).

### Acquiring and releasing pooled connections

`acquire(endpoint: Endpoint) -> Connection`

Returns an open SSH+SFTP `Connection` to the endpoint, blocking if `mc` connections are already in use and all are busy. If a previously released connection is still within its `ka` window, it is reused; otherwise a new connection is established. Establishing a new connection performs SSH handshake (bounded by `ct` seconds), authenticates (see Authentication below), and verifies the host key against `~/.ssh/known_hosts` — an unknown host causes connection failure. If the handshake or authentication fails, surface the failure as an I/O error.

`release(connection: Connection)` returns the connection to its pool. It remains idle for up to `ka` seconds; if reused within that window, the keep-alive timer resets, otherwise the underlying SSH+SFTP session is closed when the timer expires.

When verbosity is set to `trace`, each `acquire` and `release` emits one log line: `endpoint=<user@host> connections=<in_use>/<mc>`.

### Operations on a `Connection`

A `Connection` exposes the standard transport operations described in `sync.md` §"Peer Transports", performed against the remote filesystem reachable via this SSH+SFTP session. Every operation returns one of the categorized error outcomes from `sync.md` §"Error Semantics" — `not found`, `permission denied`, `I/O error` — never a transport-specific error. Path arguments are absolute remote paths.

- `list_dir(path)` — list immediate children. Each entry reports name, `is_dir`, `mod_time`, and `byte_size` (bytes for regular files, `-1` for directories). Symbolic links, devices, FIFOs, sockets, and any non-regular entry are silently omitted.
- `stat(path)` — return `mod_time`, `byte_size`, `is_dir`, or `not found` (also returned for symlinks and special files).
- `open_read(path) -> handle`, `read(handle, max_bytes) -> bytes | EOF`, `close_read(handle)` — chunked streaming read.
- `open_write(path) -> handle`, `write(handle, bytes)`, `close_write(handle)` — chunked streaming write. `open_write` creates the file and any missing parent directories.
- `rename(src, dst)` — same-filesystem rename (used by the caller for TMP-to-final swap).
- `delete_file(path)` and `delete_dir(path)` — remove a regular file or empty directory.
- `create_dir(path)` — create a directory and any missing parents.
- `set_mod_time(path, time)` — set a file or directory's modification time.

The reader/writer task pairing that streams content from one transport to another (with the bounded channel) lives above this component; this component only provides the chunk-level primitives.

### Authentication

When opening a fresh SSH connection to an endpoint, authentication is attempted in this order, stopping at the first that succeeds:
1. Inline password supplied at `open_endpoint`, if any.
2. SSH agent at `$SSH_AUTH_SOCK`.
3. `~/.ssh/id_ed25519`.
4. `~/.ssh/id_ecdsa`.
5. `~/.ssh/id_rsa`.

Host-key verification is performed against `~/.ssh/known_hosts`. An unknown host is rejected (the connection attempt fails as an I/O error).

### Shutdown

`close_endpoint(endpoint: Endpoint)` — close every idle connection in the pool and refuse subsequent `acquire` calls. The caller invokes this once per endpoint at the end of a run. In-flight operations on a `Connection` that has not yet been released complete and the connection is then closed instead of returning to the pool.

## Anchoring
- SSH transport, authentication, and channel multiplexing: RFC 4251, RFC 4252, RFC 4253, RFC 4254.
- SFTP wire protocol: `draft-ietf-secsh-filexfer` (commonly version 3 as widely deployed).
- `known_hosts` format and `~/.ssh/id_*` key file conventions: the OpenSSH specification (man pages `ssh_config(5)`, `sshd_config(5)`, `ssh-agent(1)`).
- Pool semantics (`mc`, `ct`, `ka`, lazy creation, acquire/release, trace events): `concurrency.md` §"Connection Pool (SFTP)" and §"Trace Logging".
- Authentication fallback chain and host-key verification: `sync.md` §"Authentication".
- Transport operation surface and error category set: `sync.md` §"Peer Transports", §"Required Operations", §"Error Semantics".
- `Endpoint` key (user, host, port) and SFTP URL semantics: RFC 3986 plus the SFTP scheme conventions; the `Endpoint` constructor takes already-parsed components, so URL parsing itself is not part of this component.
- Path strings, byte chunks, modification timestamps: host-language primitives.
