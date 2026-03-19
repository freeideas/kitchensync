# Sync Traversal

Multi-tree synchronization algorithm: listing, classification, decision rules, and snapshot updates.

## $REQ_SYNC_001: Parallel Peer Listing
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

At each directory level, all peers are listed in parallel.

## $REQ_SYNC_002: Union of Entry Names
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

Entry names are the union of all peer listings and snapshot children for that path.

## $REQ_SYNC_004: Classification - Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A peer's entry is classified as Unchanged when it is live with the same mod_time as the snapshot (within timestamp tolerance).

## $REQ_SYNC_005: Classification - Modified
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A peer's entry is classified as Modified when it is live with a different mod_time from the snapshot (outside timestamp tolerance), and the snapshot is also live.

## $REQ_SYNC_006: Classification - New
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A peer's entry is classified as New when it is live but the snapshot entry is absent or has a tombstone.

## $REQ_SYNC_007: Classification - Deleted
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A peer's entry is classified as Deleted when it is absent but the snapshot entry is live.

## $REQ_SYNC_008: Timestamp Tolerance
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Timestamp tolerance is 5 seconds in either direction. For classification, a peer's mod_time is "same" as the snapshot if it differs by ≤ 5 seconds. When comparing two peers' mod_times, timestamps within 5 seconds of each other are treated as equal.

## $REQ_SYNC_009: Decision - All Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If all peers are classified as unchanged, no action is taken.

## $REQ_SYNC_010: Decision - Modified Newest Wins
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When entries are modified, the newest mod_time wins. The winning version is pushed to all peers that don't match.

## $REQ_SYNC_011: Decision - New Newest Wins
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When entries are new, the newest mod_time wins. The winning version is pushed to all peers that lack it.

## $REQ_SYNC_012: Decision - Deleted Plus Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When an entry is deleted on some peers and unchanged on others, deletion wins.

## $REQ_SYNC_013: Decision - Deleted Plus Modified
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When an entry is deleted on some peers and modified on others, the modification wins.

## $REQ_SYNC_014: Decision - Same Time Different Size
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When entries have the same mod_time (within tolerance) but different sizes, the larger file wins.

## $REQ_SYNC_015: Decision - Ties Keep Data
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

On ties, data is preserved: existence wins over deletion, larger wins over smaller.

## $REQ_SYNC_016: Directory Decisions Same As Files
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

Directories use the same entry classification and decision rules (1–7) as files, including mod_time comparison and timestamp tolerance.

## $REQ_SYNC_017: Directory Displacement
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

Directories are displaced to BACK/ just like files when they lose a decision.

## $REQ_SYNC_018: Orphaned Tombstone Removal
**Source:** ./specs/multi-tree-sync.md (Section: "Orphaned Tombstones")

If an entry (file or directory) is absent on all reachable peers and exists only as a tombstone in the snapshot, the tombstone is removed.

## $REQ_SYNC_019: Type Conflict Resolution
**Source:** ./specs/multi-tree-sync.md (Section: "Type Conflicts")

When the same path is a file on one peer and a directory on another, standard decision rules apply. Since directories have byte_size −1, files win when mod_times are within tolerance. The losing entry is displaced to BACK/.

## $REQ_SYNC_020: Snapshot Updated During Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

The snapshot is updated during traversal to reflect decisions, before file copies complete. If the app exits before copies finish, the next run detects discrepancies and re-enqueues them.

## $REQ_SYNC_021: Offline Peers Excluded
**Source:** ./specs/multi-tree-sync.md (Section: "Offline Peers")

Unreachable peers are not listed. No decisions are made about their files.

## $REQ_SYNC_022: Offline Peers Catch Up
**Source:** ./specs/multi-tree-sync.md (Section: "Offline Peers")

When a previously unreachable peer becomes reachable, the snapshot reveals discrepancies and the peer is brought up to date.

## $REQ_SYNC_024: Canon Has File
**Source:** ./specs/multi-tree-sync.md (Section: "With --canon")

If the canon peer has a file, it is pushed to all other peers.

## $REQ_SYNC_025: Canon Lacks File
**Source:** ./specs/multi-tree-sync.md (Section: "With --canon")

If the canon peer lacks a file, it is deleted on all other peers.

## $REQ_SYNC_026: Canon Unreachable Exits
**Source:** ./specs/sync.md (Section: "Startup")

If the `--canon` peer is unreachable, the program exits with an error.

## $REQ_SYNC_027: Minimum Two Reachable Peers
**Source:** ./specs/sync.md (Section: "Startup")

Without `--canon`, at least two reachable peers are required.

## $REQ_SYNC_028: Canon Single Peer Sufficient
**Source:** ./specs/sync.md (Section: "Startup")

With `--canon`, one reachable peer (the canon peer itself) is sufficient. The snapshot is updated from the canon peer's state so that when other peers come online, bidirectional changes are detected and propagated.

## $REQ_SYNC_029: Filenames Preserved As-Is
**Source:** ./specs/sync.md (Section: "Case Sensitivity")

Filenames are preserved exactly as the filesystem reports them.

## $REQ_SYNC_030: Recursive Directory Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

When a decision results in a directory, the algorithm recurses into that directory after creating or deleting it on peers as needed.

## $REQ_SYNC_031: Built-in Excludes
**Source:** ./specs/multi-tree-sync.md (Section: "Built-in Excludes")

The following are always excluded from listings and never synced: `.kitchensync/` directories, symbolic links (files and directories), special files (devices, FIFOs, sockets), and `.git/` directories.
