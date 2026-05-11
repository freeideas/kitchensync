# A single SSH+SFTP session: handshake, authenticate, verify host key, run SFTP operations against the remote filesystem.

## Purpose
Open one SSH transport to a remote host, authenticate the user, verify the server's host key against `~/.ssh/known_hosts`, run the SFTP subsystem over the SSH channel, and expose the file-system operations that SFTP provides. The session has no notion of pools, endpoints, retries, or higher-level error categories — it is one open SSH+SFTP session and the operations you can run on it. Callers that need many sessions to the same account, idle reuse, or error categorization layer those concerns on top.

## API surface

### Opening a session

`open_session(host: string, port: int, user: string, credentials: ordered list of Credential, connect_timeout_secs: int) -> Session`

Establishes one SSH transport to `host:port`, performs key-exchange, verifies the server's presented host key against `~/.ssh/known_hosts` and rejects an unknown host (the open fails with an I/O failure), authenticates the user by trying each `Credential` in the supplied order and stopping at the first that succeeds, then starts the SFTP subsystem over the established SSH channel. The handshake and authentication together are bounded by `connect_timeout_secs`. If no credential authenticates, or any step fails, surface the failure as an I/O failure.

A `Credential` is one of:
- `Password(value: string)` — RFC 4252 `password` method.
- `Agent(socket_path: string)` — RFC 4252 `publickey` method, signing delegated to the SSH agent listening on the named UNIX socket.
- `PrivateKeyFile(path: string)` — RFC 4252 `publickey` method, signing performed locally with the key loaded from the named file (OpenSSH or PEM format).

### Operations on a session

Each operation takes the session plus path/handle arguments. Paths are absolute remote paths as understood by the SFTP server. Each operation returns either a success value or one of three categorized failures: `not_found`, `permission_denied`, `io_failure`. The categories are derived from SFTP status codes (`SSH_FX_NO_SUCH_FILE` → `not_found`, `SSH_FX_PERMISSION_DENIED` → `permission_denied`, all other failures including `SSH_FX_FAILURE` → `io_failure`).

- `list_dir(session, path) -> entries` — list immediate children. Each entry reports name, `is_dir`, `mod_time`, and `byte_size` (bytes for regular files, `-1` for directories). Symbolic links, devices, FIFOs, sockets, and any non-regular entry are silently omitted.
- `stat(session, path) -> { mod_time, byte_size, is_dir } | not_found` — also returns `not_found` for symbolic links and special files.
- `open_read(session, path) -> read_handle`, `read(read_handle, max_bytes) -> bytes | EOF`, `close_read(read_handle)` — chunked streaming read.
- `open_write(session, path) -> write_handle`, `write(write_handle, bytes)`, `close_write(write_handle)` — chunked streaming write. `open_write` creates the file and any missing parent directories.
- `rename(session, src, dst)` — same-filesystem rename.
- `delete_file(session, path)` — remove a regular file.
- `delete_dir(session, path)` — remove an empty directory.
- `create_dir(session, path)` — create a directory and any missing parents.
- `set_mod_time(session, path, time)` — set a file or directory's modification time.

### Closing

`close_session(session)` — close the SFTP subsystem and the underlying SSH transport. In-flight operations against this session must complete (or fail) before the session is closed.

## Anchoring
- SSH transport, key-exchange, channel multiplexing: RFC 4251, RFC 4253, RFC 4254.
- User authentication methods (`password`, `publickey`): RFC 4252.
- SFTP wire protocol, operation set, status codes (`SSH_FX_NO_SUCH_FILE`, `SSH_FX_PERMISSION_DENIED`, `SSH_FX_FAILURE`, file attribute fields): `draft-ietf-secsh-filexfer` (commonly version 3).
- Host-key verification against `~/.ssh/known_hosts` and the `known_hosts` line format (including hashed entries and wildcards): OpenSSH `sshd(8)` / `ssh_config(5)` man pages.
- Private key file formats (OpenSSH and PEM): OpenSSH `ssh-keygen(1)` and the PKCS-style PEM encodings it produces.
- SSH agent socket protocol: OpenSSH `ssh-agent(1)`.
- Host, port, user, paths, byte chunks, modification timestamps: host-language primitives.
