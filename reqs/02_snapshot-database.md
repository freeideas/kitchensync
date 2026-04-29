# 02_snapshot-database: Per-peer snapshot.db location, download/upload, schema basics

## Behavior

Each peer keeps its own snapshot in `{peer-root}/.kitchensync/snapshot.db`. At the start of a run, every peer's `snapshot.db` is downloaded to a local temporary directory; a peer that lacks one gets a fresh empty one created locally. After sync, each peer's updated snapshot is staged via `.kitchensync/TMP/<timestamp>/<uuid>/snapshot.db` and atomically renamed to `.kitchensync/snapshot.db`. Derived from `./specs/database.md` (top section, `Schema`, `Path Hashing`, `Timestamps`) and `./specs/sync.md` (`Startup` step 5, `Run` step 4).

## $REQ_IDs
- `02.41` — After a successful run, each peer has a `.kitchensync/snapshot.db` file at its root.
- `02.42` — A first-run peer that previously had no `.kitchensync/snapshot.db` ends up with one after the run completes.
- `02.43` — Snapshot reads and writes during a run go through a local copy in a temporary directory, not the peer's live `snapshot.db`.
- `02.44` — On upload, the new snapshot first appears under the peer's `.kitchensync/TMP/<timestamp>/<uuid>/snapshot.db` and is then renamed to `.kitchensync/snapshot.db`.
- `02.45` — A snapshot row records `mod_time`, `byte_size`, `last_seen`, and `deleted_time` in the form documented in `./specs/database.md`.
- `02.46` — Directories are stored in the snapshot with `byte_size = -1`.
- `02.47` — Path identifiers in snapshot rows use xxHash64 (seed 0) base62-encoded to 11 characters.
- `02.48` — Timestamps stored in the snapshot and in `BAK/`/`TMP/` directory names use the format `YYYY-MM-DD_HH-mm-ss_ffffffZ` (UTC, microsecond precision).
- `02.49` — The sync root directory itself has no snapshot row; only its descendants are tracked.
