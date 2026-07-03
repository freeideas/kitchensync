# 010_snapshot-row-updates: Snapshot row updates

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Snapshot
Updates", "Entry Classification", "Orphaned Snapshot Rows", "Directory
Decisions", and "Offline Peers", `specs/database.md` sections "Schema",
"Tombstones", "Path Hashing", and "Timestamps", and `specs/SCENARIOS.md`
scenarios S-02 through S-06 and S-10. It covers when snapshot rows are inserted
or updated for listed entries, intended copy destinations, completed copies,
created directories, confirmed absences, tombstones, displacement cascades,
offline or failed subtrees, opportunistic stale-row cleanup, and later
rediscovery of unfinished copy work.

## Notes
This category owns row-level state changes after traversal or copy events.
The SQLite file container and schema creation belong to
`004_snapshot-database-lifecycle`.

## $REQ_IDs
