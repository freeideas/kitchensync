# 02_listing-and-stat: list_dir and stat read directory entries and metadata.

## Behavior
`list_dir` and `stat` (specs/SPEC.md §"Operations on a session") are the read-only metadata operations on an open session. `list_dir` returns the immediate children of a directory, reporting `name`, `is_dir`, `mod_time`, and `byte_size` for each regular file or subdirectory; non-regular entries (symbolic links, devices, FIFOs, sockets) are silently omitted, and directories report `byte_size` as `-1`. `stat` returns `mod_time`, `byte_size`, and `is_dir` for an existing regular file or directory; it returns `not_found` for missing paths, symbolic links, and special files. Both operations follow the categorized-failure model: `SSH_FX_NO_SUCH_FILE` maps to `not_found` and `SSH_FX_PERMISSION_DENIED` maps to `permission_denied`.

## $REQ_IDs
- `02.9` — `list_dir` on an existing directory returns its immediate children.
- `02.10` — Each `list_dir` entry includes `name`, `is_dir`, `mod_time`, and `byte_size`.
- `02.11` — `list_dir` entries for directories report `byte_size` as `-1`.
- `02.12` — `list_dir` omits non-regular entries (symbolic links, devices, FIFOs, sockets).
- `02.13` — `list_dir` returns `not_found` for a missing path.
- `02.14` — `list_dir` returns `permission_denied` for a directory the user cannot read.
- `02.15` — `stat` on an existing regular file returns its `mod_time`, `byte_size`, and `is_dir` false.
- `02.16` — `stat` on an existing directory returns `is_dir` true.
- `02.17` — `stat` returns `not_found` for a missing path.
- `02.18` — `stat` returns `not_found` for a symbolic link.
- `02.32` — `stat` returns `not_found` for a non-regular special file (e.g., FIFO, device, socket).
