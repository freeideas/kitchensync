# 03_subtree-deletion: Atomically tombstoning a path and its descendants

## Behavior
`mark_subtree_deleted(handle, path, deleted_time)` atomically writes the supplied `deleted_time` onto the row at `path` and onto every transitive descendant — traced through the `parent_id → id` relationship — whose current `deleted_time` is null. Rows already carrying a non-null `deleted_time` are left untouched, preserving their original tombstone timestamp. If no row exists at `path`, the call is a no-op. Derived from `./specs/SPEC.md` § "Record operations" (mark_subtree_deleted).

## $REQ_IDs
- `03.1` — `mark_subtree_deleted` sets the row at `path`'s `deleted_time` to the supplied timestamp when that row's current `deleted_time` is null.
- `03.2` — `mark_subtree_deleted` sets `deleted_time` to the supplied timestamp on every transitive descendant of `path` whose current `deleted_time` is null.
- `03.3` — Rows whose `deleted_time` is already non-null when `mark_subtree_deleted` is called retain their existing `deleted_time` value.
- `03.4` — `mark_subtree_deleted` is a no-op (no rows changed) when no row exists at `path`.
