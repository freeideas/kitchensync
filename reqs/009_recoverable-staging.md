# 009_recoverable-staging: Recoverable staging and cleanup

## Behavior
This concern derives from `specs/sync.md` sections "Rename Compatibility",
"File Copy", "Displace to BAK", "TMP Staging", "SWAP Directory", "BAK
Directory", and "Errors", `specs/multi-tree-sync.md` sections "SWAP Recovery
During Traversal", "BAK/TMP Cleanup During Traversal", "Directory Decisions",
and "Type Conflicts", `specs/database.md` sections "Database" and "Snapshot
SWAP recovery", and `specs/SCENARIOS.md` scenarios S-05, S-06, and S-09. It
covers the observable use of SWAP `new` and `old` paths for user-file
replacement, recovery of incomplete user-file swaps, same-filesystem
displacement of files and directories to BAK, BAK and TMP timestamp directory
placement, cleanup retention rules, failure behavior around staging, and the
requirement not to depend on rename-over-existing behavior.

## Notes
Snapshot database replacement uses the same SWAP pattern, but the database file
lifecycle is owned by `004_snapshot-database-lifecycle`.

## $REQ_IDs
