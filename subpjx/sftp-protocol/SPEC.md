# SFTP Protocol

A Java 21 library for accessing remote filesystems over SSH File Transfer
Protocol (SFTP). It provides authenticated SSH/SFTP sessions, host-key
verification, directory and file operations, streaming read/write handles, and a
thread-safe transfer connection pool keyed by SSH endpoint.

The library is for SFTP filesystem access only. It does not implement a sync
algorithm, file conflict decisions, snapshots, ignore rules, local filesystem
access, command-line parsing, fallback URL selection, peer roles, BAK/TMP
policies, or peer-to-peer copy orchestration. It also does not write diagnostics
to stdout or stderr; callers receive results, errors, and optional pool events
through the public API.

## Public API

The API may use normal Java classes, records, interfaces, or equivalent types,
but it must expose this behavior.

### Data Shapes

`SftpLocation`

| Field | Meaning |
| --- | --- |
| `user` | SSH username. Required. |
| `password` | Optional inline password. |
| `host` | SSH hostname, stored lowercase for endpoint keys. |
| `port` | SSH port. Omitted/default SFTP port is normalized to `22`. |
| `root_path` | Absolute remote filesystem path used as the root for relative operations. |

`SftpSettings`

| Field | Meaning |
| --- | --- |
| `max_connections` | Maximum open pooled transfer connections for one endpoint. Positive integer. |
| `connect_timeout` | SSH handshake timeout for each connection attempt. Positive duration. |
| `idle_keep_alive_ttl` | How long an idle pooled connection remains open before real close. Positive duration. |

`AuthConfig`

| Field | Meaning |
| --- | --- |
| `known_hosts_path` | OpenSSH `known_hosts` file. Default: `~/.ssh/known_hosts`. |
| `ssh_agent_socket` | Optional SSH agent socket. Default: `SSH_AUTH_SOCK` when set. |
| `private_key_paths` | Private keys tried after the agent. Default order: `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, `~/.ssh/id_rsa`. |

Unknown host keys are rejected. There is no public option to accept unknown
hosts silently.

`Entry`

| Field | Meaning |
| --- | --- |
| `name` | Final path component only. |
| `is_dir` | `true` for a directory, `false` for a regular file. |
| `mod_time` | Remote modification time as a UTC `Instant` using the best precision reported by the server. |
| `byte_size` | File size in bytes, or `-1` for directories. |

`SftpError`

| Category | Meaning |
| --- | --- |
| `not_found` | Path does not exist, or exists only as a symlink or special file. |
| `permission_denied` | Authentication succeeded, but the server denied the requested filesystem operation. |
| `io_error` | Network failures, timeouts after connection start, server disconnects, protocol failures, and other read/write failures. |
| `authentication_failed` | No configured authentication method succeeded. |
| `host_key_rejected` | Host key is absent from or mismatches `known_hosts`. |
| `invalid_path` | Caller supplied an absolute path, a `..` segment, a NUL byte, or another path outside the bound root. |

Filesystem operations on an established `SftpFilesystem` return only
`not_found`, `permission_denied`, `io_error`, or `invalid_path`. Session creation
may additionally return `authentication_failed` or `host_key_rejected`.

### Paths

Public filesystem operations use slash-separated paths relative to
`SftpLocation.root_path`.

- Empty string means the bound root directory.
- Paths must not start with `/`.
- Paths must not contain `..` as a segment.
- Paths must not contain NUL.
- The implementation must not follow symlinks.

### Session Operations

`SftpConnector.open_unpooled(location, settings, auth_config) -> SftpFilesystem`

Opens one SSH+SFTP session using `settings.connect_timeout`. The session is not
part of any transfer pool and is closed when its `SftpFilesystem` is closed.
This operation is suitable for startup checks and directory listing.

Authentication attempts are made in this order:

1. `location.password`, when present
2. SSH agent from `auth_config.ssh_agent_socket`
3. Private keys from `auth_config.private_key_paths` in order

Host keys are verified against `auth_config.known_hosts_path` before
authentication is accepted.

### Filesystem Operations

`SftpFilesystem` exposes:

| Operation | Behavior |
| --- | --- |
| `list_dir(path) -> List<Entry>` | Lists immediate children only. Returns regular files and directories. Omits symlinks, devices, FIFOs, sockets, and other special entries. |
| `stat(path) -> Entry` | Returns one entry for a regular file or directory. Returns `not_found` for symlinks and special entries. |
| `open_read(path) -> ReadHandle` | Opens a regular file for streaming read. |
| `read(handle, max_bytes) -> bytes or EOF` | Reads up to `max_bytes`. EOF is distinct from an empty byte array. |
| `close_read(handle)` | Closes a read handle. It is safe to call after a failed read. |
| `open_write(path) -> WriteHandle` | Opens a regular file for streaming write, creating missing parent directories as needed. Existing files may be truncated according to SFTP server behavior. |
| `write(handle, bytes)` | Writes bytes to the open handle. |
| `close_write(handle)` | Flushes and closes the write handle. |
| `rename(src, dst)` | Renames within the same remote filesystem. Creates no parent directories. |
| `delete_file(path)` | Removes a regular file. |
| `create_dir(path)` | Creates the directory and any missing parents. Succeeds if the directory already exists. |
| `delete_dir(path)` | Removes an empty directory. |
| `set_mod_time(path, instant)` | Sets modification time on a file or directory as precisely as the server supports. |

The library does not implement a complete file-copy operation. Callers compose
`open_read`, `read`, `open_write`, `write`, and close operations into whatever
copy pipeline they require.

### Transfer Pool

`SftpPoolRegistry`

Creates and owns transfer pools. A pool key is `user@host:port`, with host
lowercased and omitted/default port normalized to `22`. The root path and
password are not part of the key.

`pool_for(location, settings, auth_config, pool_listener) -> SftpTransferPool`

- Creates the pool lazily on the first call for a key.
- If a pool already exists for the key, returns the existing pool.
- The first call for a key supplies that pool's `max_connections`,
  `idle_keep_alive_ttl`, authentication data, and listener. Later calls for the
  same key do not change those values.

`SftpTransferPool.acquire() -> PooledSftpFilesystem`

- Returns an idle connection if one is available.
- Otherwise opens a new SSH+SFTP connection if fewer than `max_connections` are
  open.
- If `max_connections` connections are already open and all are busy, waits
  until one is released or the caller cancels/interruption occurs.
- Each new SSH handshake uses `connect_timeout`.

`PooledSftpFilesystem.close()`

Returns the connection to the pool instead of closing the underlying SSH+SFTP
session immediately. An idle returned connection remains alive for up to
`idle_keep_alive_ttl`; reuse resets the timer. When the timer expires, the
underlying session is closed.

If an operation leaves a pooled connection unusable, the library closes that
connection instead of returning it to the idle set. Failed operations must not
leak permits or permanently reduce pool capacity.

`SftpPoolRegistry.close()`

Closes all idle and borrowed sessions owned by the registry. Closing a registry
is idempotent.

### Pool Events

When a listener is supplied, the library emits a pool event on every acquire,
release, and idle-timeout close:

| Field | Meaning |
| --- | --- |
| `endpoint` | `user@host:port` pool key. |
| `open_connections` | Current number of open SSH+SFTP sessions in the pool. |
| `max_connections` | Pool limit. |

A caller that wants trace logging can format each event exactly as:

```
endpoint=<user@host:port> connections=<open>/<max>
```

## Observable Behavior

- SSH handshake timeout applies to each connection attempt.
- Connection pools are thread-safe.
- Distinct `SftpFilesystem` instances may be used concurrently.
- A single read or write handle is not required to be safe for concurrent use by
  multiple threads.
- Directory listings never include symlinks or special files.
- `stat` on a symlink or special file returns `not_found`.
- `open_write` creates missing parent directories.
- `create_dir` is recursive and idempotent when the target already exists as a
  directory.
- Network drops and SFTP protocol failures are reported as `io_error`.
- Public operations do not print to stdout or stderr.

## Examples

### Basic Filesystem Access

Input:

```java
SftpLocation loc = new SftpLocation(
    "ace",
    Optional.empty(),
    "ordinarydata.com",
    22,
    "/tmp/testks/example-basic");

SftpSettings settings = new SftpSettings(10, Duration.ofSeconds(30), Duration.ofSeconds(30));
AuthConfig auth = AuthConfig.defaults();

try (SftpFilesystem fs = SftpConnector.open_unpooled(loc, settings, auth)) {
    fs.create_dir("");
    try (WriteHandle out = fs.open_write("notes/hello.txt")) {
        fs.write(out, "hello\n".getBytes(StandardCharsets.UTF_8));
    }
    fs.set_mod_time("notes/hello.txt", Instant.parse("2026-05-15T10:30:00Z"));
    List<Entry> entries = fs.list_dir("notes");
}
```

Observable output from `list_dir("notes")`:

```text
Entry(name="hello.txt", is_dir=false, mod_time=2026-05-15T10:30:00Z, byte_size=6)
```

The reported `mod_time` may be rounded by the SFTP server or remote filesystem.

### Shared Transfer Pool

Input:

```java
SftpPoolRegistry pools = new SftpPoolRegistry();

SftpLocation a = new SftpLocation("ace", Optional.empty(), "ordinarydata.com", 22, "/tmp/testks/a");
SftpLocation b = new SftpLocation("ace", Optional.empty(), "ORDINARYDATA.COM", 22, "/tmp/testks/b");
SftpSettings settings = new SftpSettings(2, Duration.ofSeconds(30), Duration.ofSeconds(30));

SftpTransferPool poolA = pools.pool_for(a, settings, AuthConfig.defaults(), listener);
SftpTransferPool poolB = pools.pool_for(b, settings, AuthConfig.defaults(), listener);

boolean samePool = poolA == poolB;
try (PooledSftpFilesystem first = poolA.acquire();
     PooledSftpFilesystem second = poolB.acquire()) {
    first.stat("");
    second.stat("");
}
```

Concrete results:

```text
samePool = true
endpoint=ace@ordinarydata.com:22 connections=1/2
endpoint=ace@ordinarydata.com:22 connections=2/2
endpoint=ace@ordinarydata.com:22 connections=2/2
endpoint=ace@ordinarydata.com:22 connections=2/2
```

Both locations share one pool because the pool key ignores path and normalizes
host case and default port.

## Testing Requirements

Black-box tests must exercise the public API against an SFTP server. The
required remote test account is:

```text
sftp://ace@ordinarydata.com/tmp/testks/
```

The local account running the tests must be able to authenticate to that server
without a password. Each test must create a unique child directory under
`/tmp/testks/` and clean up files it creates when cleanup is possible.

Required scenarios:

- Passwordless authentication through the configured default chain.
- Host-key verification succeeds for a known host and rejects an unknown or
  mismatched host key.
- `create_dir` creates missing parent directories and is idempotent.
- `open_write`, `write`, `close_write`, `open_read`, `read`, and `close_read`
  round-trip binary content without text conversion.
- `set_mod_time`, `stat`, and `list_dir` report regular file metadata, allowing
  for server filesystem timestamp precision.
- `list_dir` and `stat` omit symlinks and special files when the test account can
  create them; skip only the special-file creation part when server permissions
  prevent it.
- `rename`, `delete_file`, and `delete_dir` perform the requested remote
  filesystem changes.
- Missing paths return `not_found`; permission failures, when a readable fixture
  can be arranged, return `permission_denied`; forced disconnects or unreachable
  endpoints return `io_error` or connection failure categories as appropriate.
- Two locations with the same user, lowercased host, and normalized port share a
  pool even when their root paths differ.
- A pool with `max_connections = 1` blocks a second acquire until the first
  borrowed connection is closed.
- Idle pooled connections are actually closed after `idle_keep_alive_ttl`.
- Pool events report `endpoint`, current open connection count, and max count on
  acquire, release, and idle-timeout close.
- A failed pooled operation does not leak capacity; a later acquire can still
  reach `max_connections`.

Scenarios to avoid:

- Do not test multi-peer sync decisions, conflict resolution, snapshots,
  tombstones, ignore files, BAK directories, TMP staging policy, or local
  filesystem behavior in this component.
- Do not rely on wall-clock ordering tighter than the remote filesystem's
  timestamp precision.
- Do not require the SFTP server to support privileged operations.

## Semantic Anchors

This specification is anchored in:

- SSH Transport Layer Protocol, RFC 4253
- SSH Connection Protocol, RFC 4254
- SSH File Transfer Protocol behavior as implemented by common SFTP servers
- The semantic source sections for SFTP peer transport, authentication,
  transport operations, built-in symlink/special-file exclusion, SFTP connection
  pooling, trace pool events, and the required SFTP test account
