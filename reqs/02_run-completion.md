# 02_run-completion: Run lifecycle, snapshot upload, and exit code

## Behavior

After the combined-tree walk, KitchenSync waits for all enqueued file copies to complete, uploads each peer's updated snapshot back via TMP staging and atomic rename, disconnects, and exits 0. Derived from `specs/sync.md` §"Run" and `specs/database.md`.

## $REQ_IDs
- `02.43` — All enqueued file copies finish before any peer's snapshot is uploaded back.
- `02.44` — Each peer's updated `snapshot.db` is first written to `{peer-root}/.kitchensync/TMP/<timestamp>/<uuid>/snapshot.db`.
- `02.48` — The staged `snapshot.db` is then renamed to `{peer-root}/.kitchensync/snapshot.db` via a same-filesystem (atomic) rename.
- `02.45` — A successful sync run exits with code 0.

## Notes
Snapshot upload failure handling is in `04_error-handling.md`. Startup-purge of stale snapshot rows is in `03_snapshot-tombstones.md`.
