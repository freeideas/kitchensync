# 02_file-ops-mutations: Rename, delete, directory create/remove, set modification time

## Behavior

A connection handle exposes mutating filesystem operations against the remote: `rename(src, dst)` performs a same-filesystem rename of a file or directory; `delete_file(path)` removes a regular file; `create_dir(path)` creates a directory along with any missing parents and is idempotent if the directory already exists; `delete_dir(path)` removes an empty directory; `set_mod_time(path, time)` sets the modification time of a file or directory. Derives from `specs/SPEC.md` § "API surface > File operations".

## $REQ_IDs

- `02.35` — `rename(src, dst)` renames a regular file from `src` to `dst` on the same remote filesystem.
- `02.36` — `rename(src, dst)` renames a directory from `src` to `dst` on the same remote filesystem.
- `02.37` — `delete_file(path)` removes a regular file at `path`.
- `02.38` — `create_dir(path)` creates the directory at `path`, including any missing parent directories along the way.
- `02.39` — `create_dir(path)` succeeds without error when the directory at `path` already exists (idempotent).
- `02.40` — `delete_dir(path)` removes an empty directory at `path`.
- `02.41` — `set_mod_time(path, time)` sets the modification time of the file or directory at `path` to the given value, observable on a subsequent `stat` or `list_dir`.

## Notes

- Paths are absolute and use forward-slash separators.
