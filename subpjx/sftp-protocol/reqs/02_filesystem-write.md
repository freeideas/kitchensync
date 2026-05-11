# 02_filesystem-write: Chunked streaming write, rename, delete, create_dir, set_mod_time

## Behavior

A `Connection` exposes write-side filesystem primitives against the remote SSH+SFTP filesystem. `open_write` / `write` / `close_write` provide chunked streaming writes; `open_write` creates the file and any missing parent directories. `rename(src, dst)` performs a same-filesystem rename (used by the caller for TMP-to-final swap). `delete_file` and `delete_dir` remove a regular file or an empty directory respectively. `create_dir(path)` creates a directory and any missing parents. `set_mod_time(path, time)` sets the modification time of a file or directory. Derived from `SPEC.md` §"Operations on a `Connection`".

## $REQ_IDs
- `02.30` — `open_write(path)` returns a write handle, `write(handle, bytes)` appends bytes to the file, and `close_write(handle)` finalizes it.
- `02.31` — `open_write` creates the file and any missing parent directories.
- `02.32` — `rename(src, dst)` performs a same-filesystem rename of the entry at `src` to `dst`.
- `02.33` — `delete_file(path)` removes a regular file at `path`.
- `02.34` — `delete_dir(path)` removes an empty directory at `path`.
- `02.35` — `create_dir(path)` creates a directory at `path` and any missing parent directories.
- `02.36` — `set_mod_time(path, time)` sets the modification time of the file or directory at `path` to `time`.
