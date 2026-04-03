# Sync Decision Rules

How KitchenSync decides what action to take for each file entry.

## $REQ_DEC_001: Canon Wins Unconditionally
**Source:** ./specs/algorithm.md (Section: "Decision Rules - With a canon peer")

When a canon peer (`+`) is present: if canon has the file, it is pushed to all others. If canon lacks the file, it is deleted everywhere else (displaced to BAK/).

## $REQ_DEC_002: All Unchanged No Action
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

If all contributing voters classify the entry as Unchanged, no action is taken.

## $REQ_DEC_003: Newest Mod-Time Wins
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

When live entries exist and no deletions, the entry with the newest mod_time wins and is pushed to peers that need it. Peers within 5-second tolerance of the max are considered tied.

## $REQ_DEC_004: Tie-Break by File Size
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

When mod_times are tied (within 5-second tolerance), the larger file wins. If all peers agree on both mod_time and byte_size, no copy is needed.

## $REQ_DEC_005: All Deleted Means Delete
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

If all voters have deleted the entry (no live copies), the entry is deleted on all peers.

## $REQ_DEC_006: Deletion vs Existence Comparison
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

When both live and deleted entries exist, the max deletion estimate is compared to the max live mod_time. If the deletion estimate exceeds the live mod_time by more than 5 seconds, deletion wins. Otherwise, the existing file wins and is pushed.

## $REQ_DEC_007: Ties Favor Existence
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

When deletion estimate and live mod_time are within 5-second tolerance, existence wins (the file is kept and pushed).

## $REQ_DEC_008: Absent-Unconfirmed Handling
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

Absent-unconfirmed entries (absent, row exists, no tombstone): if `last_seen` is NULL (pending copy that never completed), treat as needing the file. If `last_seen` exceeds all live mod_times by more than 5 seconds, treat as confirmed deletion. Otherwise, treat as needing the file (re-enqueue copy).

## $REQ_DEC_013: Ghost Entries Resolve to Delete
**Source:** ./specs/algorithm.md (Section: "Decision Rules - Without a canon peer")

If after absent-unconfirmed classification all entries in the live set are not physically present on any peer (pending copies that never completed), the decision is DELETE.

## $REQ_DEC_009: No Contributing Peer Has Entry
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

If no contributing peer has the entry (all are No-Opinion), subordinates with the entry are displaced (DELETE_SUBORDINATES_ONLY). Contributing peer snapshots are not modified.

## $REQ_DEC_010: Skip Unnecessary Copies
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

If the winning entry already exists on a peer with matching mod_time (within tolerance) and matching byte_size, no copy is performed -- only the snapshot row is updated.

## $REQ_DEC_011: Directory Decisions Are Existence-Based
**Source:** ./specs/algorithm.md (Section: "Directory Decisions")

Directories do not use mod_time for decisions. If any contributing peer has the directory, it is created on peers that lack it. If all contributing peers have deleted it (tombstone + absent), it is deleted everywhere (displaced to BAK/).

## $REQ_DEC_012: Type Conflict Resolution
**Source:** ./specs/algorithm.md (Section: "Type Conflicts")

When the same path is a file on one peer and a directory on another: if a canon peer is present, canon's type wins. Without a canon peer, the file type wins and the directory is displaced to BAK/.
