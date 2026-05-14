# SFTP File Streams

## Purpose
Provide streaming file reads and writes over an established root-bound SFTP session.

## Public API
Data shapes:

- `Session`: established SSH+SFTP session with a peer root path
- `ReadHandle`
- `WriteHandle`

Operations:

- `open_read(session, path) -> ReadHandle`
- `read(session, handle, max_bytes) -> bytes | EOF`
- `close_read(session, handle)`
- `open_write(session, path) -> WriteHandle`
- `write(session, handle, bytes)`
- `close_write(session, handle)`

All operation paths are relative to the session peer root path.

## Behavior
`open_read` opens an existing file for streaming reads. `read` returns up to `max_bytes` bytes or `EOF`. `close_read` closes the read handle.

`open_write` creates the target file and missing parent directories as needed. `write` appends bytes to the open write handle. `close_write` completes and closes the write handle.

## Errors
File stream operations return only these categories:

- `not_found`
- `permission_denied`
- `io_error`

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

If an operation cannot complete because of permissions, it returns `permission_denied`. Other transport or filesystem failures return `io_error`.

`open_read` on a missing path returns `not_found`.

## Anchoring
`Session` is anchored in SSH transport/session behavior from RFC 4253 and RFC 4254 and SFTP filesystem semantics from `draft-ietf-secsh-filexfer`.

`open_read`, `read`, `close_read`, `open_write`, `write`, `close_write`, `ReadHandle`, `WriteHandle`, `bytes`, and `EOF` are anchored in `sync.md` "Peer Transports" and SFTP file read/write semantics from `draft-ietf-secsh-filexfer`.

`path` and peer-root-relative path behavior are anchored in `sync.md` "Peer Transports".

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
