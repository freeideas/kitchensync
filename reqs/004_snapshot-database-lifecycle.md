# 004_snapshot-database-lifecycle: Snapshot database lifecycle

## Behavior
This concern derives from `specs/database.md` sections "Database", "Schema",
and "Snapshot SWAP recovery", `specs/sync.md` sections "Startup", "Run", and
"Rename Compatibility", and `specs/SCENARIOS.md` properties "P-04: Snapshot
Upload Is Atomic Through SWAP" and "P-05: Dry Run Does Not Write Peer State".
It covers the observable location of each peer's `snapshot.db`, rollback
journal storage, exact snapshot table and indexes, local temporary database
creation, snapshot download, closed-file upload readiness, atomic snapshot
replacement through SWAP, recovery of incomplete snapshot replacement, and
dry-run snapshot upload suppression.

## $REQ_IDs

- `004.1` -- Each peer stores its snapshot database at `{peer-root}/.kitchensync/snapshot.db`.
- `004.2` -- Each created or updated snapshot database is a SQLite database in rollback-journal mode.
- `004.3` -- KitchenSync treats only `.kitchensync/snapshot.db` as peer snapshot state and does not sync SQLite sidecar files for that database.
- `004.4` -- A created snapshot database contains exactly one application table named `snapshot`.
- `004.5` -- A created snapshot database contains no view or alternate table name for the snapshot table.
- `004.6` -- The `snapshot` table has an `id TEXT` primary key column.
- `004.7` -- The `snapshot` table has a `parent_id TEXT` column.
- `004.8` -- The `snapshot` table has a `basename TEXT NOT NULL` column.
- `004.9` -- The `snapshot` table has a `mod_time TEXT NOT NULL` column.
- `004.10` -- The `snapshot` table has a `byte_size INTEGER NOT NULL` column.
- `004.11` -- The `snapshot` table has a nullable `last_seen TEXT` column.
- `004.12` -- The `snapshot` table has a nullable `deleted_time TEXT` column.
- `004.13` -- The snapshot schema has a non-primary index on `snapshot(parent_id)`.
- `004.14` -- The snapshot schema has a non-primary index on `snapshot(last_seen)`.
- `004.15` -- The snapshot schema has a non-primary index on `snapshot(deleted_time)`.
- `004.16` -- In a normal run, KitchenSync recovers incomplete `.kitchensync/SWAP/snapshot.db/` state on each reachable peer before downloading that peer's snapshot database.
- `004.17` -- In a normal run, KitchenSync downloads each reachable peer's `.kitchensync/snapshot.db` to a local temporary `{tmp}/{uuid}/snapshot.db` before reading or updating that snapshot.
- `004.18` -- When a reachable peer has no `.kitchensync/snapshot.db`, KitchenSync creates a new empty local temporary snapshot database for that peer.
- `004.19` -- If snapshot SWAP recovery or snapshot download fails for a reason other than a missing `.kitchensync/snapshot.db`, KitchenSync logs an error-level diagnostic and excludes that peer from the reachable set.
- `004.20` -- During a sync run, KitchenSync performs snapshot reads and writes against each peer's local temporary `snapshot.db`.
- `004.21` -- In a normal run, KitchenSync starts snapshot uploads only after all enqueued file copies have completed.
- `004.22` -- Before uploading a local temporary snapshot database, KitchenSync closes that `snapshot.db` as a self-contained file with no required SQLite sidecar.
- `004.23` -- In a normal run, KitchenSync uploads a peer snapshot replacement by writing and closing `.kitchensync/SWAP/snapshot.db/new`.
- `004.24` -- In a normal run, KitchenSync moves an existing live `.kitchensync/snapshot.db` to `.kitchensync/SWAP/snapshot.db/old` before moving `new` into the live path.
- `004.25` -- In a normal run, KitchenSync moves `.kitchensync/SWAP/snapshot.db/new` to `.kitchensync/snapshot.db` to publish the uploaded snapshot.
- `004.26` -- After a successful normal snapshot replacement, KitchenSync deletes `.kitchensync/SWAP/snapshot.db/old`.
- `004.27` -- Snapshot replacement does not require a transport rename onto an existing destination path.
- `004.28` -- If a normal snapshot upload fails after `.kitchensync/SWAP/snapshot.db/old` exists, KitchenSync leaves the incomplete snapshot SWAP state on the peer.
- `004.29` -- When normal startup finds both `.kitchensync/SWAP/snapshot.db/old` and live `.kitchensync/snapshot.db`, KitchenSync deletes `.kitchensync/SWAP/snapshot.db/new` if present and then deletes `.kitchensync/SWAP/snapshot.db/old`.
- `004.30` -- When normal startup finds `.kitchensync/SWAP/snapshot.db/old` and `.kitchensync/SWAP/snapshot.db/new` but no live `.kitchensync/snapshot.db`, KitchenSync renames `new` to the live snapshot path and then deletes `old`.
- `004.31` -- When normal startup finds `.kitchensync/SWAP/snapshot.db/old` but neither `.kitchensync/SWAP/snapshot.db/new` nor live `.kitchensync/snapshot.db`, KitchenSync renames `old` to the live snapshot path.
- `004.32` -- When normal startup finds `.kitchensync/SWAP/snapshot.db/new` and live `.kitchensync/snapshot.db` but no `.kitchensync/SWAP/snapshot.db/old`, KitchenSync deletes `new`.
- `004.33` -- When normal startup finds `.kitchensync/SWAP/snapshot.db/new` but neither `.kitchensync/SWAP/snapshot.db/old` nor live `.kitchensync/snapshot.db`, KitchenSync renames `new` to the live snapshot path.
- `004.34` -- In `--dry-run`, KitchenSync skips peer-side snapshot SWAP recovery.
- `004.35` -- In `--dry-run`, KitchenSync downloads the live peer `.kitchensync/snapshot.db` as-is.
- `004.36` -- In `--dry-run`, KitchenSync creates and updates local temporary snapshot databases as local working state.
- `004.37` -- In `--dry-run`, KitchenSync does not create, modify, rename, delete, or upload peer snapshot state through a peer URL.

## Notes
This category owns the database file and schema as a peer artifact. Row
meaning during reconciliation belongs to `010_snapshot-row-updates`.
