# 02_filesystem-read: Directory listing, stat, and chunked streaming read

## Behavior

A `Connection` exposes read-side filesystem primitives against the remote SSH+SFTP filesystem. `list_dir(path)` enumerates immediate children with name, `is_dir`, `mod_time`, and `byte_size`; non-regular entries (symlinks, devices, FIFOs, sockets, etc.) are silently omitted. `stat(path)` returns `mod_time`, `byte_size`, and `is_dir`, or `not found` for missing paths, symlinks, and special files. `open_read` / `read` / `close_read` provide chunked streaming reads of regular files. Derived from `SPEC.md` §"Operations on a `Connection`".

## $REQ_IDs
- `02.20` — `list_dir(path)` returns the immediate children of the directory at `path`.
- `02.21` — Each `list_dir` entry includes `name`, `is_dir`, `mod_time`, and `byte_size`; `byte_size` is the file size in bytes for regular files and `-1` for directories.
- `02.22` — `list_dir` silently omits symbolic links, devices, FIFOs, sockets, and any other non-regular entries.
- `02.23` — `stat(path)` returns `mod_time`, `byte_size`, and `is_dir` for an existing regular file or directory.
- `02.24` — `stat(path)` returns `not found` for a non-existent path, for symbolic links, and for special files.
- `02.25` — After `open_read(path)`, repeated `read(handle, max_bytes)` calls followed by `close_read(handle)` reproduce the regular file's bytes in order.
- `02.26` — `read` returns `EOF` once all bytes of the underlying file have been delivered.
- `02.27` — Each `read(handle, max_bytes)` call returns at most `max_bytes` bytes.
