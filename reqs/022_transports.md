# 022_transports: Transport operations and error semantics

## Behavior
This concern derives from `specs/sync.md` section "Peer Transports" (Required
Operations, Error Semantics, and the symlink/special-file omission rule).

It covers the uniform filesystem interface every transport (`file://` and
`sftp://`) must provide to the sync engine and that both schemes behave
identically to it: the required operations (`list_dir`, `stat`, `open_read`,
`read`, `close_read`, `open_write`, `write`, `close_write`, `rename`,
`delete_file`, `create_dir`, `delete_dir`, `set_mod_time`) with their specified
shapes - including `list_dir` returning name, `is_dir`, `mod_time`, and
`byte_size` (-1 for directories), `rename` requiring a non-existent destination,
and `open_write`/`create_dir` creating parents as needed. It covers that
`list_dir` and `stat` silently omit symbolic links, special files, and any
non-regular entry (`stat` returns "not found" for them). It covers the common
error categories all operations return - not found, permission denied, I/O
error - with network failures surfacing as I/O errors and sync logic never
matching on transport-specific errors.

URL-scheme parsing is `001_command-line` and URL identity is
`003_url-normalization`. Connecting a transport is `005_connection-establishment`.
The streaming layer built on these chunk primitives is `020_copy-execution`.

## $REQ_IDs
- `022.1` -- A `file://` peer and an `sftp://` peer with identical directory contents yield identical sync results.
- `022.2` -- `list_dir(path)` returns each immediate child's name, `is_dir`, `mod_time`, and `byte_size`.
- `022.3` -- `list_dir(path)` reports `byte_size` as the file size in bytes for a regular file.
- `022.4` -- `list_dir(path)` reports `byte_size` as -1 for a directory.
- `022.5` -- `stat(path)` returns `mod_time`, `byte_size`, and `is_dir` for an existing regular file or directory.
- `022.6` -- `stat(path)` returns "not found" when the path does not exist.
- `022.7` -- `read(handle, max_bytes)` returns the next chunk of bytes, or EOF at the end of the file.
- `022.8` -- `open_write(path)` creates the target file and any missing parent directories.
- `022.9` -- `create_dir(path)` creates the directory and any missing parent directories.
- `022.10` -- `rename(src, dst)` moves `src` to `dst` when `dst` does not exist.
- `022.11` -- `rename(src, dst)` fails when `dst` already exists.
- `022.12` -- `delete_file(path)` removes a file.
- `022.13` -- `delete_dir(path)` removes an empty directory.
- `022.14` -- `set_mod_time(path, time)` sets the modification time of a file or directory.
- `022.15` -- `list_dir(path)` silently omits symbolic links, special files, and any other non-regular entry.
- `022.16` -- `stat(path)` returns "not found" for a symbolic link or special file.
- `022.17` -- Every operation reports failures using only the categories not found, permission denied, and I/O error, regardless of transport scheme.
- `022.18` -- A network failure such as a connection drop or timeout surfaces as an I/O error.
- `022.19` -- An I/O failure produces the same sync handling whether it occurs on a `file://` peer or an `sftp://` peer.
