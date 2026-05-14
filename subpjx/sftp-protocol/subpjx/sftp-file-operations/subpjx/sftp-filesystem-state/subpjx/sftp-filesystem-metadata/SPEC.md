# SFTP Filesystem Metadata

## Purpose
Provide SFTP directory listing and metadata lookup over an established root-bound SFTP session.

## Public API
Data shapes:

- `Session`: established SSH+SFTP session with a peer root path
- `Entry`: `name`, `is_dir`, `mod_time`, `byte_size`
- `Stat`: `is_dir`, `mod_time`, `byte_size`

Operations:

- `list_dir(session, path) -> Entry[]`
- `stat(session, path) -> Stat`

All operation paths are relative to the session peer root path.

## Behavior
`list_dir` returns only immediate regular-file and directory children. Symbolic links, devices, FIFOs, sockets, and other special entries are omitted.

`stat` reports regular files and directories. `stat` reports symbolic links and special entries as not found.

## Errors
Metadata operations return only these categories:

- `not_found`
- `permission_denied`
- `io_error`

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

`stat` on a symbolic link or special file returns `not_found`.

## Anchoring
`Session` is anchored in SSH transport/session behavior from RFC 4253 and RFC 4254 and SFTP filesystem semantics from `draft-ietf-secsh-filexfer`.

`list_dir` and `stat` are anchored in `sync.md` "Peer Transports".

`Entry`, `Stat`, `mod_time`, `byte_size`, regular-file filtering, symbolic-link handling, and special-file handling are anchored in `sync.md` "Peer Transports" and `ignore.md` "Symlinks" / "Built-in Excludes".

`path` and peer-root-relative path behavior are anchored in `sync.md` "Peer Transports" and SFTP path semantics from `draft-ietf-secsh-filexfer`.

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
