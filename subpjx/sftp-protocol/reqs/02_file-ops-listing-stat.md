# 02_file-ops-listing-stat: Directory listing and entry stat

## Behavior

A connection handle exposes `list_dir(path)` to enumerate the immediate children of a directory and `stat(path)` to inspect a single entry. Both operations only report regular files and directories; non-regular entry types (symlinks, devices, FIFOs, sockets) are silently omitted from listings and reported as "not found" by `stat`. Derives from `specs/SPEC.md` § "API surface > File operations".

## $REQ_IDs

- `02.19` — `list_dir(path)` returns the immediate children of the directory at `path`.
- `02.20` — Each `list_dir` entry exposes `name`, `is_dir` (boolean), `mod_time` (UTC timestamp), and `byte_size`.
- `02.21` — `byte_size` is the file size in bytes for regular files, and `-1` for directory entries.
- `02.22` — `stat(path)` returns `(mod_time, byte_size, is_dir)` for an existing regular file or directory.
- `02.23` — `stat(path)` reports "not found" when no entry exists at `path`.
- `02.24` — `list_dir` omits non-regular entries (symlinks, devices, FIFOs, sockets) from its output.
- `02.25` — `stat` reports "not found" for non-regular entry types.

## Notes

- Paths are absolute and use forward-slash separators (per `specs/SPEC.md` § "File operations" preamble).
