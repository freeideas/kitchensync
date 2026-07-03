# SQLite Snapshot

## Risk

The specs require each peer snapshot to be a SQLite `snapshot.db` in
rollback-journal mode. The product must update the exact single-table schema,
run a recursive CTE to tombstone a displaced subtree, and upload only a closed,
self-contained `snapshot.db` file.

## Experiment

`experiments/sqlite-snapshot` is a Rust mini-project using:

- `rusqlite` `0.32.1`

It opens a temporary `snapshot.db` with `rusqlite::Connection::open`, sets
`PRAGMA journal_mode=DELETE`, creates the `snapshot` table and indexes, inserts
a directory row, child file row, and unrelated row, then runs the recursive CTE
from the spec through `Connection::execute` and `params!`.

The experiment drops the connection, checks that no `snapshot.db-journal`,
`snapshot.db-wal`, or `snapshot.db-shm` sidecar remains, and verifies the file
header is `SQLite format 3`.

## Proved Calls

- `Connection::open(&path)` opens a peer snapshot file.
- `Connection::query_row("PRAGMA journal_mode=DELETE", [], ...)` selects
  rollback-journal mode and returns `delete`.
- `Connection::execute_batch(...)` creates the schema and indexes.
- `Connection::transaction`, `Transaction::execute`, and `Transaction::commit`
  insert snapshot rows.
- `Connection::execute` runs the recursive CTE and returns the changed row
  count.
- After all statements and the connection are dropped, the database file is
  standalone on this machine.

## Notes

No custom Cargo features were needed for `rusqlite` in this experiment.
