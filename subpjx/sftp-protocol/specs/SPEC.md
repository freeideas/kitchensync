# sftp-protocol

A pooled SFTP client library: file operations over SSH, with connection reuse keyed by user+host.

## Purpose

Implement the client side of the SSH File Transfer Protocol over SSH transport, exposing a uniform set of file operations against a remote filesystem. Multiple operations against the same (user, host) destination are multiplexed across a bounded pool of reused SSH+SFTP sessions with configurable maximum concurrency, handshake timeout, and idle keep-alive. A caller acquires a connection handle, issues file operations through it, and releases the handle when done; the pool reuses sessions across acquisitions.

## API surface

### Pool

The component is instantiated as a pool. Each pool is keyed by an `(sftp-user, host)` pair: every URL that resolves to the same user and host shares the same pool, regardless of path. Distinct ports are distinct pool keys (port is part of the host identity for pool keying).

Pool configuration:

- `max_connections` (default 10) — maximum number of concurrent live SSH+SFTP sessions to this `(user, host)` pair.
- `connect_timeout_seconds` (default 30) — bounds the SSH handshake on a single connection attempt; expiry is a connection failure.
- `idle_keepalive_seconds` (default 30) — released-but-idle sessions remain warm for this many seconds before the underlying SSH+SFTP session is torn down; reuse before the timer expires resets it.

Pool operations:

- `acquire(url)` → connection handle. Returns an idle session if one is cached for the URL's user+host; otherwise opens a new SSH+SFTP session up to `max_connections`. If `max_connections` are already open and all are busy, blocks until one is released. If the URL specifies a username, that user is used; if the URL has no username, the current OS user is used.
- `release(handle)` — return the handle to the pool. The session is held alive for up to `idle_keepalive_seconds`; if not reused within that window, the underlying SSH+SFTP session is closed.
- `shutdown()` — close every cached and in-use session and tear the pool down.

A consumer that needs two connections simultaneously (e.g., a streaming copy from one remote to another) acquires once per destination; if both destinations resolve to the same `(user, host)`, both handles come from the same pool and both count against its `max_connections`.

### Authentication

When opening a new SSH+SFTP session, authentication is attempted in this order; the first method that succeeds wins:

1. Inline password from the URL (if present)
2. SSH agent (via the `SSH_AUTH_SOCK` environment variable)
3. `~/.ssh/id_ed25519`
4. `~/.ssh/id_ecdsa`
5. `~/.ssh/id_rsa`

### Host key verification

Server host keys are verified against the user's `~/.ssh/known_hosts` file (OpenSSH format). Connections to hosts whose key is not present, or whose key does not match the recorded entry, are rejected as connection failures.

### File operations

A connection handle exposes the following operations against the remote filesystem. Paths are absolute, rooted at the remote's filesystem root, and use forward-slash separators.

| Operation | Description |
| --- | --- |
| `list_dir(path)` | List the immediate children of a directory. Each entry has: `name`, `is_dir` (bool), `mod_time` (UTC timestamp), `byte_size` (file size in bytes; `-1` for directories). |
| `stat(path)` | Return `(mod_time, byte_size, is_dir)` for the entry at `path`, or report "not found". |
| `open_read(path)` → read handle | Open a regular file for streaming reads. |
| `read(handle, max_bytes)` | Pull the next chunk; return the bytes read, or EOF. |
| `close_read(handle)` | Close a read handle. |
| `open_write(path)` → write handle | Open a regular file for streaming writes. Create the file if absent; create any missing parent directories. |
| `write(handle, bytes)` | Push the next chunk. |
| `close_write(handle)` | Flush and close a write handle. |
| `rename(src, dst)` | Same-filesystem rename of a file or directory on the remote. |
| `delete_file(path)` | Remove a regular file. |
| `create_dir(path)` | Create a directory and any missing parents (idempotent if the directory already exists). |
| `delete_dir(path)` | Remove an empty directory. |
| `set_mod_time(path, time)` | Set the modification time of a file or directory. |

`list_dir` and `stat` only report regular files and directories. Symbolic links, devices, FIFOs, sockets, and any other non-regular entry types are silently omitted from `list_dir`; `stat` reports "not found" for them.

### Error categories

Every operation surfaces failures in exactly three categories, regardless of the underlying SSH/SFTP status code:

- **not found** — the named path does not exist.
- **permission denied** — the remote refused the operation for authorization reasons.
- **I/O error** — anything else: network drop, handshake timeout, session collapse, protocol error, remote write failure, server-side error, etc.

Network failures (connection reset, handshake timeout, SFTP channel death) surface as I/O errors; callers do not need to distinguish transport-level failures from on-disk failures.

## Anchoring

- **SFTP wire protocol**: `draft-ietf-secsh-filexfer` — the IETF SSH File Transfer Protocol draft.
- **SSH transport and connection**: RFC 4253 (SSH Transport Layer Protocol) and RFC 4254 (SSH Connection Protocol).
- **SSH authentication methods**: RFC 4252 (SSH Authentication Protocol); RFC 8709 (Ed25519 public keys), RFC 5656 (ECDSA public keys), RFC 4253 §6.6 (RSA public keys).
- **SSH agent**: SSH Agent Protocol (draft-miller-ssh-agent), located via the `SSH_AUTH_SOCK` environment variable.
- **`known_hosts` file format**: the OpenSSH known_hosts convention (publicly documented).
- **URL form for endpoints**: RFC 3986 (URI Generic Syntax) — the `sftp://[user[:password]@]host[:port]/path` form.
- **Connection pool semantics**: a standard bounded resource pool with idle keep-alive — a well-known concurrency abstraction.
- **Filesystem operation surface**: the operations mirror POSIX filesystem primitives (`open`, `read`, `write`, `close`, `stat`, `readdir`, `rename`, `unlink`, `mkdir`, `rmdir`, `utime`) — the standard surface every UNIX-style filesystem exposes.
