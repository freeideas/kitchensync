# 05_is-ignored-entry: Filesystem-entry-kind dispatch wraps is_ignored

## Behavior
`is_ignored_entry(m, path, kind)` is a thin wrapper over `is_ignored`. For `file` and `dir` it delegates to `is_ignored` with the corresponding `is_dir` value. For `symlink` and `special` it short-circuits to true regardless of the matcher's user rules. Derived from SPEC.md §"Querying" (`is_ignored_entry` paragraph).

## $REQ_IDs
- `05.1` — `is_ignored_entry(m, path, file)` returns the same value as `is_ignored(m, path, false)`.
- `05.2` — `is_ignored_entry(m, path, dir)` returns the same value as `is_ignored(m, path, true)`.
- `05.3` — `is_ignored_entry(m, path, symlink)` returns true for any `m` and any `path`.
- `05.4` — `is_ignored_entry(m, path, special)` returns true for any `m` and any `path`.
