# 02_record-queries: Looking up rows and listing a directory's children

## Behavior
`lookup(handle, path)` returns the row stored for `path` if one exists, otherwise no record. `list_children(handle, parent_path)` returns every row whose `parent_id` equals `identify(parent_path)`; passing `/` or the empty string lists the immediate children of the root sentinel. These read operations expose the state written by the record-write operations. Derived from `./specs/SPEC.md` § "Record operations".

## $REQ_IDs
- `02.1` — `lookup(handle, path)` returns the row previously written at `path`, with the fields that were written.
- `02.2` — `lookup(handle, path)` returns no record when no row exists at `path`.
- `02.3` — `list_children(handle, parent_path)` returns every row whose `parent_id` equals `identify(parent_path)`.
- `02.4` — `list_children(handle, "/")` returns the rows for the root's immediate children (those whose `parent_id` is the root-sentinel identity).
- `02.5` — `list_children(handle, "")` returns the same set of rows as `list_children(handle, "/")`.
