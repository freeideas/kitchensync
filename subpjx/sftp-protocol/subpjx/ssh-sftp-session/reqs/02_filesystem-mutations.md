# 02_filesystem-mutations: rename, delete_file, delete_dir, create_dir, and set_mod_time mutate the remote filesystem.

## Behavior
These operations (specs/SPEC.md §"Operations on a session") mutate the remote filesystem through the SFTP subsystem. `rename` performs a same-filesystem rename of a path. `delete_file` removes a regular file. `delete_dir` removes an empty directory. `create_dir` creates a directory and any missing parent directories. `set_mod_time` sets the modification time on a file or directory. Each operation surfaces `not_found`, `permission_denied`, or `io_failure` per the session's categorized-failure model.

## $REQ_IDs
- `02.25` — `rename` moves a path from `src` to `dst`: the entry is no longer accessible at `src` and is accessible at `dst`.
- `02.26` — `delete_file` removes a regular file so it is no longer accessible at its path.
- `02.27` — `delete_dir` removes an empty directory so it is no longer accessible at its path.
- `02.28` — `create_dir` creates a directory at the given path.
- `02.29` — `create_dir` creates missing parent directories along the path.
- `02.30` — `set_mod_time` updates the modification time reported by `stat` for a regular file.
- `02.31` — `set_mod_time` updates the modification time reported by `stat` for a directory.
