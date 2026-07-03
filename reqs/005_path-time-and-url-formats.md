# 005_path-time-and-url-formats: Path, time, and URL formats

## Behavior
This concern derives from `specs/database.md` sections "URL Normalization",
"Path Hashing", and "Timestamps", `specs/sync.md` sections "Command-Line
Excludes", "TMP Staging", "SWAP Directory", and "BAK Directory", and
`specs/multi-tree-sync.md` sections "Decision Rules", "Directory Decisions",
"Snapshot Updates", and "BAK/TMP Cleanup During Traversal". It covers the
observable normalization of peer URLs, relative slash-path rules, snapshot row
ID and parent ID format, metadata basename encoding, UTC timestamp string
format, per-process monotonic generated timestamps, copied deletion timestamp
semantics, and the five-second comparison tolerance.

## Notes
This category owns shared data formats and comparisons. The database schema
that stores these values belongs to `004_snapshot-database-lifecycle`, and the
decisions that consume them belong to `007_reconciliation-decisions`.

## $REQ_IDs
