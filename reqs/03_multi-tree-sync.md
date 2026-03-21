# Multi-Tree Synchronization

Combined-tree walk algorithm, entry classification, decision rules, and special cases.

## $REQ_MTS_001: Parallel Listing Per Level
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

At each directory level, all peers' directories are listed in parallel.

## $REQ_MTS_002: Union of Entry Names
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

Entry names are unioned across all active peers to form the set of entries to decide on.

## $REQ_MTS_003: Listing Error Excludes Peer from Subtree
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

If `list_directory` fails for a specific path on a reachable peer, that peer is excluded from decisions for that directory and its entire subtree. The error is logged at `error` level. The peer's snapshot rows for that subtree are not modified.

## $REQ_MTS_004: Live Entries Only Drive Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "Overview")

The snapshot is consulted per-peer for reconciliation but does not contribute entries to the union — only live peer listings drive traversal.

## $REQ_MTS_005: Built-in Exclude .kitchensync
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

`.kitchensync/` directories are always excluded from listings and never synced.

## $REQ_MTS_006: Built-in Exclude Symbolic Links
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

Symbolic links (files and directories) are always excluded from listings.

## $REQ_MTS_007: Built-in Exclude Special Files
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

Special files (devices, FIFOs, sockets) are always excluded from listings.

## $REQ_MTS_008: Built-in Exclude .git
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

`.git/` directories are always excluded from listings.

## $REQ_MTS_009: Entry Classification — Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A live entry with the same mod_time as its snapshot row (within tolerance) and `deleted_time` NULL is classified as unchanged.

## $REQ_MTS_010: Entry Classification — Modified
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A live entry with a different mod_time than its snapshot row and `deleted_time` NULL is classified as modified.

## $REQ_MTS_011: Entry Classification — Resurrection
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A live entry where the snapshot row has `deleted_time` NOT NULL is classified as modified (resurrection). The `deleted_time` is cleared.

## $REQ_MTS_012: Entry Classification — New
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A live entry with no snapshot row for this peer is classified as new.

## $REQ_MTS_013: Entry Classification — Deleted
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An absent entry where the snapshot row has `deleted_time` NOT NULL is classified as deleted, with the deletion estimate being `deleted_time`.

## $REQ_MTS_014: Entry Classification — Absent-Unconfirmed
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An absent entry where the snapshot row has `deleted_time` NULL is classified as absent-unconfirmed.

## $REQ_MTS_015: Decision — Canon Peer Has File
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

With a canon peer, the canon peer's state wins unconditionally. If canon has a file, it is pushed to all others.

## $REQ_MTS_016: Decision — Canon Peer Lacks File
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

With a canon peer, if canon lacks a file, it is deleted everywhere else.

## $REQ_MTS_036: Decision — Canon Unreachable
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

With a canon peer, if the canon peer is unreachable, the sync exits with an error at startup.

## $REQ_MTS_017: Decision — All Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without canon, if all entries are unchanged, no action is taken (rule 1).

## $REQ_MTS_018: Decision — Modified Newest Wins
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without canon, a modified entry with the newest mod_time wins and is pushed to all peers that don't match (rule 2).

## $REQ_MTS_019: Decision — New Entry
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without canon, a new entry with the newest mod_time wins and is pushed to all peers that lack it, including peers with no snapshot row (rule 3).

## $REQ_MTS_020: Decision — Deletion vs Existing
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without canon, when comparing a deletion estimate against an existing file's mod_time: if deletion estimate exceeds mod_time (beyond tolerance), deletion wins. If mod_time is greater than or equal to the deletion estimate (within tolerance), the existing file wins and is pushed to peers that lack it (rule 4).

## $REQ_MTS_021: Decision — Absent-Unconfirmed Resolution
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

For absent-unconfirmed entries: if `last_seen` exceeds the max mod_time of peers that have the entry by more than 5 seconds, it is treated as a deletion (rule 4b). If `last_seen` is less than or equal to max mod_time (within tolerance) or NULL, the copy is re-enqueued with no deletion vote.

## $REQ_MTS_022: Decision — Same Mod Time Different Size
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When entries have the same mod_time (within tolerance) but different sizes, the larger file wins (rule 5).

## $REQ_MTS_023: Decision — Ties
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

On ties, existence wins over deletion, and larger wins over smaller (rule 6).

## $REQ_MTS_024: Timestamp Tolerance
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Timestamp tolerance is 5 seconds in either direction for all mod_time comparisons in entry classification and decision rules. A peer whose mod_time is within 5 seconds of the maximum is treated as tied.

## $REQ_MTS_025: No Copy When Already Matching
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the winning entry already exists on a peer with matching mod_time (within tolerance) and matching byte_size, no copy is performed — only the snapshot row is updated.

## $REQ_MTS_026: No-Opinion Peers
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Peers with no snapshot row for an entry ("never had it") do not vote — they are targets for propagation once a winner is decided.

## $REQ_MTS_027: Directory Decisions Same as Files
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

Directories use the same entry classification and decision rules as files (mod_time comparison, newest wins, timestamp tolerance). Directories are displaced to BACK/ just like files.

## $REQ_MTS_028: Directory Creation Sets Mod Time
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

When a directory is created on a peer, its mod_time is set to match the source directory's mod_time.

## $REQ_MTS_029: Type Conflict Resolution
**Source:** ./specs/multi-tree-sync.md (Section: "Type Conflicts")

When the same path is a file on one peer and a directory on another, standard decision rules apply. Since directories have byte_size −1, files win when mod_times are within tolerance (rule 6). The losing entry is displaced to BACK/.

## $REQ_MTS_030: Inline Displacement
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

All displacement (type conflicts, deletions) executes during the combined-tree walk, not in the operation queue. Displacement is a same-filesystem rename to BACK/.

## $REQ_MTS_031: No Recursion Into Displaced Directories
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

A directory being displaced on a peer is not recursed into. The displacement moves the entire subtree in a single rename. Only peers keeping the directory participate in recursion.

## $REQ_MTS_032: BACK/XFER Cleanup During Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "BACK/XFER Cleanup During Traversal")

After processing entries at each directory level, each peer is checked for a `.kitchensync/` directory at the current path. If present, expired entries in `BACK/` (older than `back-retention-days`) and `XFER/` (older than `xfer-cleanup-days`) are purged. The timestamp in the subdirectory name determines age.

## $REQ_MTS_034: Offline Peers Excluded
**Source:** ./specs/multi-tree-sync.md (Section: "Offline Peers")

Unreachable peers are excluded entirely from listings and decisions. Their snapshot rows are not modified. On the next run when reachable, discrepancies drive sync decisions.

## $REQ_MTS_035: Multiple Deletion Estimates
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If multiple peers have deleted an entry, the most recent deletion estimate among the deleting peers is used for comparison in rule 4.
