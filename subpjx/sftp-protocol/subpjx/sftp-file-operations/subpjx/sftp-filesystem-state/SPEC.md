# SFTP Filesystem State

## Purpose
Provide SFTP directory listing, metadata, and file/directory state changes over an established root-bound SFTP session.

## Public API
Data shapes:

- `Session`: established SSH+SFTP session with a peer root path
- `Entry`: `name`, `is_dir`, `mod_time`, `byte_size`
- `Stat`: `is_dir`, `mod_time`, `byte_size`

Operations:

- `list_dir(session, path) -> Entry[]`
- `stat(session, path) -> Stat`
- `rename(session, src, dst)`
- `delete_file(session, path)`
- `create_dir(session, path)`
- `delete_dir(session, path)`
- `set_mod_time(session, path, time)`

All operation paths are relative to the session peer root path.

## Behavior
`list_dir` returns only immediate regular-file and directory children. Symbolic links, devices, FIFOs, sockets, and other special entries are omitted.

`stat` reports regular files and directories. `stat` reports symbolic links and special entries as not found.

`rename` is a same-filesystem rename. `delete_file` removes a file. `create_dir` creates a directory. `delete_dir` removes an empty directory. `set_mod_time` sets the modification time supplied by the caller.

## Errors
Filesystem state operations return only these categories:

- `not_found`
- `permission_denied`
- `io_error`

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

`stat` on a symbolic link or special file returns `not_found`.

If a rename, delete, directory creation, or mod-time update cannot complete because of permissions, it returns `permission_denied`. Other transport or filesystem failures return `io_error`.

## Anchoring
`Session` is anchored in SSH transport/session behavior from RFC 4253 and RFC 4254 and SFTP filesystem semantics from `draft-ietf-secsh-filexfer`.

`list_dir`, `stat`, `rename`, `delete_file`, `create_dir`, `delete_dir`, and `set_mod_time` are anchored in `sync.md` "Peer Transports".

`Entry`, `Stat`, `mod_time`, `byte_size`, regular-file filtering, symbolic-link handling, and special-file handling are anchored in `sync.md` "Peer Transports" and `ignore.md` "Symlinks" / "Built-in Excludes".

`path`, `src`, `dst`, and peer-root-relative path behavior are anchored in `sync.md` "Peer Transports" and SFTP path semantics from `draft-ietf-secsh-filexfer`.

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
