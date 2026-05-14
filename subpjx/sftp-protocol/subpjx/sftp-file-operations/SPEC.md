# SFTP File Operations

## Purpose
Provide SFTP filesystem operations over an established root-bound SFTP session.

## Public API
Data shapes:

- `Session`: established SSH+SFTP session with a peer root path
- `Entry`: `name`, `is_dir`, `mod_time`, `byte_size`
- `Stat`: `is_dir`, `mod_time`, `byte_size`
- `ReadHandle`
- `WriteHandle`

Operations:

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

All operation paths are relative to the session peer root path.

## Behavior
`list_dir` returns only immediate regular-file and directory children. Symbolic links, devices, FIFOs, sockets, and other special entries are omitted.

`stat` reports regular files and directories. `stat` reports symbolic links and special entries as not found.

`open_read` opens an existing file for streaming reads. `read` returns up to `max_bytes` bytes or `EOF`. `close_read` closes the read handle.

`open_write` creates the target file and missing parent directories as needed. `write` appends bytes to the open write handle. `close_write` completes and closes the write handle.

`rename` is a same-filesystem rename. `delete_file` removes a file. `create_dir` creates a directory. `delete_dir` removes an empty directory. `set_mod_time` sets the modification time supplied by the caller.

## Errors
Filesystem operations return only these categories:

- `not_found`
- `permission_denied`
- `io_error`

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

`stat` on a symbolic link or special file returns `not_found`.

If a write, close, rename, delete, directory creation, or mod-time update cannot complete because of permissions, it returns `permission_denied`. Other transport or filesystem failures return `io_error`.

## Anchoring
`Session` is anchored in SSH transport/session behavior from RFC 4253 and RFC 4254 and SFTP filesystem semantics from `draft-ietf-secsh-filexfer`.

`list_dir`, `stat`, streaming handles, `rename`, `delete_file`, `create_dir`, `delete_dir`, and `set_mod_time` are anchored in `sync.md` "Peer Transports".

`Entry`, `Stat`, `mod_time`, `byte_size`, regular-file filtering, symbolic-link handling, and special-file handling are anchored in `sync.md` "Peer Transports" and `ignore.md` "Symlinks" / "Built-in Excludes".

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
