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

## Notes
This category owns the database file and schema as a peer artifact. Row
meaning during reconciliation belongs to `010_snapshot-row-updates`.

## $REQ_IDs
