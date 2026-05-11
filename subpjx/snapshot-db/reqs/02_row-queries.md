# 02_row-queries: Read snapshot rows by id and list child rows by parent id.

## Behavior
Callers consult the snapshot before traversal acts: looking up a single row by its id, and listing all rows directly under a given parent id. A missing row yields nothing; a parent with no children yields an empty list. Both queries see every row currently in the table, including tombstones (rows with `deleted_time` set). Derived from `SPEC.md` §"Row operations".

## $REQ_IDs
- `02.10` — Looking up a row by an id that has no stored row returns no row (a missing/absent result).
- `02.11` — Looking up a row by id after it has been written returns a row whose `parent_id`, `basename`, `mod_time`, `byte_size`, `last_seen`, and `deleted_time` fields match what is stored.
- `02.12` — Listing child rows of a parent id returns every row whose `parent_id` equals that id.
- `02.13` — Listing child rows of an id with no children returns an empty result.

## Notes
The list operation is the caller's way of learning which paths the snapshot remembers under a directory; it includes tombstoned rows so callers can act on prior-known-but-now-absent entries.
