# Sync Walk

Combined-tree walk: directory traversal, entry union, and pre-order processing.

## $REQ_WALK_001: Pre-Order Traversal
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

The traversal is pre-order: every entry in a directory is decided and acted on before recursing into any subdirectory. A directory marked for displacement is renamed (with its entire subtree) before its children are ever visited.

## $REQ_WALK_002: Parallel Listing Per Level
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

At each directory level, all peers are listed in parallel. Peers with listing errors are excluded from the entire subtree.

## $REQ_WALK_003: Union of Entry Names
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

Entry names are unioned across all contributing peers. Subordinate peers' entries are also included in the union (they participate in listing but not in decisions).

## $REQ_WALK_004: No Contributing Peers at Level
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

If no contributing peer is available at a directory level, a warning is logged and the subtree is skipped.

## $REQ_WALK_005: .syncignore Resolved First
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

If `.syncignore` appears in the union, it is decided first (winning version determined, copies enqueued). The winning version's patterns are merged with parent rules. If the read fails, a warning is logged and parent-level rules are used.

## $REQ_WALK_006: Filtered Entries Skipped
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

Entries matching accumulated ignore rules are skipped -- no decisions, no copies, no snapshot updates.

## $REQ_WALK_007: BAK/TMP Cleanup Per Level
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

At each directory level, expired BAK/ entries (older than `--bd` days) and expired TMP/ entries (older than `--xd` days) are cleaned up. Age is determined from the timestamp directory name, not filesystem modification time.

## $REQ_WALK_008: Cleanup Removes Entire Timestamp Directory
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

`cleanup_expired` deletes entire timestamp directories (and all contents) when the timestamp is older than the threshold. For TMP, this includes nested UUID directories.

## $REQ_WALK_009: Entry Classification
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

Each file entry on each contributing peer is classified by comparing filesystem state to snapshot: Unchanged (live, same mod_time), Modified (live, different mod_time), Resurrection (live, had tombstone), New (live, no row), Deleted (absent, has tombstone), Absent-unconfirmed (absent, row exists, no tombstone), or No opinion (absent, no row).

## $REQ_WALK_010: Mod-Time Tolerance
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

"Same mod_time" means within 5-second tolerance. This tolerance applies to classification, decision comparisons, and rule 4b.
