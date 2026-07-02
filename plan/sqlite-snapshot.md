# SQLite Snapshot

## Risk

Each peer stores `.kitchensync/snapshot.db` as a single SQLite file in rollback
journal mode. Product code must create the exact table and indexes, close the
database before upload, avoid WAL sidecars, and use the recursive CTE for
directory displacement tombstones.

## Experiment

`plan/experiments/sqlite-snapshot` uses `rusqlite` `0.32.1` with bundled SQLite.
It opens a temporary `snapshot.db`, runs `PRAGMA journal_mode=DELETE`, creates
the exact `snapshot` table columns, creates the `parent_id`, `last_seen`, and
`deleted_time` indexes, inserts a small tree, and runs the recursive CTE from
the spec:

- the displaced directory row and its child row get the copied
  `deleted_time`;
- an unrelated sibling row remains unchanged;
- after the connection is dropped, `snapshot.db` exists and
  `snapshot.db-wal`, `snapshot.db-shm`, and `snapshot.db-journal` do not.

## Proven Package

- `rusqlite` `0.32.1` with feature `bundled`

## Notes For Later Code

Use `Connection::open`, `Connection::execute_batch`, `Connection::execute`, and
`query_row` for the schema and cascade. Finish all statements and drop every
`Connection` before handing the file to a transport upload.

