# 01_store-lifecycle: Database files open, persist, and close

## Behavior
The library opens a SQLite database at a caller-supplied filesystem path, creating and initializing the file when it does not exist and reusing it when it does. Rows written through one handle remain visible after `close` and a subsequent `open` of the same file. Derived from `./specs/SPEC.md` § "Store lifecycle" and § "Record shape".

## $REQ_IDs
- `01.1` — `open(file)` creates a new database file at the given filesystem path when the file does not exist.
- `01.2` — The database created by `open(file)` contains a table named `snapshot`.
- `01.3` — Reopening the same file in a later `open(file)` call exposes the rows written before the prior `close(handle)`.

## Notes
The spec also requires indexes on `parent_id`, `last_seen`, and `deleted_time`; these are pure performance details with no observable functional effect, so they are not separately asserted.
