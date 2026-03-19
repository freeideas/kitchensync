# Multi-Tree Synchronization

Combined-tree walk algorithm, built-in excludes, entry classification, and decision rules.

## $REQ_MTS_001: Parallel Listing and Union
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

At each directory level, all active peers are listed in parallel, and their entries are unioned to form the set of names to process.

## $REQ_MTS_002: Listing Error Peer Exclusion
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

If `list_directory` fails for a specific path on a reachable peer, that peer is excluded from decisions for that directory and its entire subtree (equivalent to an offline peer for that path). The error is logged at `error` level.

## $REQ_MTS_003: No False Deletions on Listing Error
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

When a peer has a listing error, its snapshot rows for that subtree are not modified — `last_seen` is not updated, so no false deletions are inferred.

## $REQ_MTS_004: Exclude Kitchensync Directories
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

`.kitchensync/` directories are always excluded from listings and never synced.

## $REQ_MTS_005: Exclude Symbolic Links
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

Symbolic links (files and directories) are always excluded from listings and never synced.

## $REQ_MTS_006: Exclude Special Files
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

Special files (devices, FIFOs, sockets) are always excluded from listings and never synced.

## $REQ_MTS_007: Exclude Git Directories
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

`.git/` directories are always excluded from listings and never synced.

## $REQ_MTS_008: Entry Classification - Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An entry is classified as "unchanged" when it is live on a peer with the same mod_time as its snapshot row and `deleted_time` is NULL.

## $REQ_MTS_009: Entry Classification - Modified
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An entry is classified as "modified" when it is live on a peer with a different mod_time than its snapshot row and `deleted_time` is NULL.

## $REQ_MTS_010: Entry Classification - Resurrection
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An entry is classified as "modified (resurrection)" when it is live on a peer and its snapshot row has `deleted_time` NOT NULL. The `deleted_time` is cleared.

## $REQ_MTS_011: Entry Classification - New
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An entry is classified as "new" when it is live on a peer and no snapshot row exists for that peer.

## $REQ_MTS_012: Entry Classification - Deleted
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An entry is classified as "deleted" when it is absent on a peer, a snapshot row exists with `deleted_time` NOT NULL.

## $REQ_MTS_013: Entry Classification - Absent Unconfirmed
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

An entry is classified as "absent-unconfirmed" when it is absent on a peer, a snapshot row exists with `deleted_time` NULL. This triggers rule 4b for decision-making.

## $REQ_MTS_014: Decision Rule - All Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If all peers classify an entry as unchanged, no action is taken.

## $REQ_MTS_015: Decision Rule - Modified Newest Wins
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When an entry is modified, the peer with the newest mod_time wins. The winning entry is pushed to all peers that don't match.

## $REQ_MTS_016: Decision Rule - New Entry
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When an entry is new, the peer with the newest mod_time wins. The entry is pushed to all peers that lack it, including peers with no snapshot row.

## $REQ_MTS_017: Decision Rule - Deleted vs Existing
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When an entry is deleted on some peers and exists on others, the deletion estimate is compared against the existing file's mod_time. If the deletion estimate exceeds the mod_time, deletion wins (displace on all peers that have it). If mod_time is greater than or equal to the deletion estimate, the existing file wins (push to peers that lack it).

## $REQ_MTS_018: Decision Rule - Absent Unconfirmed
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

For absent-unconfirmed entries: if `last_seen` exceeds the max mod_time of peers that have the entry, this is a deletion — apply rule 4 using `last_seen` as the deletion estimate. If `last_seen` is less than or equal to max mod_time (or is NULL), this is a failed copy — re-enqueue the copy, no deletion vote.

## $REQ_MTS_019: Decision Rule - Same Mod Time Different Size
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When entries have the same mod_time but different sizes, the larger file wins.

## $REQ_MTS_020: Decision Rule - Ties
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

On ties, data is preserved: existence wins over deletion, larger wins over smaller.

## $REQ_MTS_021: No-Vote Peers
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Peers with no snapshot row for an entry do not vote — they are targets for propagation once a winner is decided.

## $REQ_MTS_022: Skip Copy When Already Matching
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the winning entry already exists on a peer with a matching mod_time (within tolerance) and matching byte_size, no copy is performed — only the snapshot row is created/updated.

## $REQ_MTS_023: Timestamp Tolerance
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Timestamp tolerance is 5 seconds in either direction. A peer's mod_time within 5 seconds of the snapshot row's mod_time is considered "same" in entry classification. When comparing peers' mod_times, any peer within 5 seconds of the maximum is treated as tied. The same tolerance applies when comparing deletion estimates against file mod_times.

## $REQ_MTS_024: Directory Decision Rules
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

Directories use the same entry classification and decision rules as files (mod_time comparison, newest wins, timestamp tolerance). Directories are displaced to BACK/ just like files.

## $REQ_MTS_025: Directory Creation Sets Mod Time
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

When a directory is created on a peer, the mod_time of the source directory (the winner) is applied to the new directory.

## $REQ_MTS_026: Type Conflict Resolution
**Source:** ./specs/multi-tree-sync.md (Section: "Type Conflicts")

When the same path is a file on one peer and a directory on another, standard decision rules apply. Since directories have byte_size −1, files win when mod_times are within tolerance (rule 6). The losing entry is displaced to BACK/.

## $REQ_MTS_027: Inline Displacement
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

All displacement (type conflicts, deletions) executes during the combined-tree walk, not in the operation queue. Displacement is a same-filesystem rename to BACK/.

## $REQ_MTS_028: Directory Deletion No Recurse
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

Do not recurse into a directory that is being displaced on a peer. The displacement moves the entire subtree in a single rename. Only peers keeping the directory participate in recursion.

## $REQ_MTS_029: Snapshot Drives No Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "Overview")

The snapshot is consulted per-peer for reconciliation but does not contribute entries to the union — only live peer listings drive traversal.

## $REQ_MTS_030: Multiple Deletion Estimates
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If multiple peers have deleted an entry, the most recent deletion estimate among the deleting peers is used for the comparison in rule 4.

## $REQ_MTS_031: BACK XFER Cleanup During Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "BACK/XFER Cleanup During Traversal")

After processing entry names at each directory level, each peer is checked for a `.kitchensync/` directory at the current path. If present, expired entries in `BACK/` (older than `back-retention-days`) and `XFER/` (older than `xfer-cleanup-days`) subdirectories are purged. The `<timestamp>` component of each subdirectory name determines its age.

## $REQ_MTS_032: List Dir Entry Format
**Source:** ./specs/sync.md (Section: "Required Operations")

`list_dir` returns immediate children with name, is_dir, mod_time, and byte_size. byte_size is file size in bytes for files, or −1 for directories.

## $REQ_MTS_033: Canon Mode - Canon Has Entry
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When `--canon` is specified, the canonical peer's state wins unconditionally. If the canon peer has an entry, it is pushed to all other peers.

## $REQ_MTS_034: Canon Mode - Canon Lacks Entry
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When `--canon` is specified and the canon peer lacks an entry, the entry is deleted (displaced to BACK/) on all other peers that have it.
