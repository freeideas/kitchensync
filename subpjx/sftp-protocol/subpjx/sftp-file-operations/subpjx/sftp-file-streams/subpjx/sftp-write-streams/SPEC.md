# SFTP Write Streams

## Purpose
Provide streaming file writes over an established root-bound SFTP session.

## Public API
Data shapes:

- `Session`: established SSH+SFTP session with a peer root path
- `WriteHandle`

Operations:

- `open_write(session, path) -> WriteHandle`
- `write(session, handle, bytes)`
- `close_write(session, handle)`

All operation paths are relative to the session peer root path.

## Behavior
`open_write` creates the target file and missing parent directories as needed.

`write` appends bytes to the open write handle.

`close_write` completes and closes the write handle.

## Errors
Write stream operations return only these categories:

- `not_found`
- `permission_denied`
- `io_error`

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

If an operation cannot complete because of permissions, it returns `permission_denied`. Other transport or filesystem failures return `io_error`.

## Anchoring
`Session` is anchored in SSH transport/session behavior from RFC 4253 and RFC 4254 and SFTP filesystem semantics from `draft-ietf-secsh-filexfer`.

`open_write`, `write`, `close_write`, `WriteHandle`, and `bytes` are anchored in `sync.md` "Peer Transports" and SFTP file write semantics from `draft-ietf-secsh-filexfer`.

`path` and peer-root-relative path behavior are anchored in `sync.md` "Peer Transports".

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
