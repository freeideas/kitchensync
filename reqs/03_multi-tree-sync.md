# Multi-Tree Synchronization

Combined-tree walk algorithm, entry classification, decision rules, directory decisions, type conflicts, snapshot updates, canon and subordinate peer behavior during sync.

## $REQ_MTS_001: Parallel Listing and Union
**Source:** ./specs/multi-tree-sync.md (Section: "Overview")

At each directory level: list all peers in parallel, union their entries, decide the authoritative state for each, act, and recurse.

## $REQ_MTS_002: Snapshot Does Not Drive Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "Overview")

The snapshot is consulted per-peer for reconciliation (detecting deletions and modifications) but does not contribute entries to the union — only live peer listings drive traversal.

## $REQ_MTS_003: Listing Error Excludes Peer from Subtree
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

If `list_directory` fails for a specific path on a reachable peer, that peer is excluded from decisions for that directory and its entire subtree. The error is logged at `error` level. The peer's snapshot rows for that subtree are not modified.

## $REQ_MTS_004: Subordinate Entries Not in Decisions
**Source:** ./specs/multi-tree-sync.md (Section: "Subordinate Peers")

A subordinate peer's entries are not included in the `gather_states` step — decisions are made as if the subordinate peer doesn't exist.

## $REQ_MTS_005: Subordinate Brought into Conformance
**Source:** ./specs/multi-tree-sync.md (Section: "Subordinate Peers")

After a decision is made, the subordinate peer is brought into conformance: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it, directories are created or removed as needed.

## $REQ_MTS_006: Subordinate Snapshot Updated
**Source:** ./specs/multi-tree-sync.md (Section: "Subordinate Peers")

A subordinate peer's snapshot is downloaded, updated during traversal, and uploaded back. On future runs without `-`, the peer participates normally.

## $REQ_MTS_008: No Contributing Votes Displaces Subordinate Files
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If no contributing peer votes (all have "absent, no row"), the entry does not exist in the group's view. Subordinate peers that have the entry are displaced to BAK/.

## $REQ_MTS_009: Entry Classification - Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A file is classified as Unchanged when: live on peer, snapshot row exists with `deleted_time = NULL`, and mod_time matches (within 5-second tolerance).

## $REQ_MTS_010: Entry Classification - Modified
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A file is classified as Modified when: live on peer, snapshot row exists with `deleted_time = NULL`, and mod_time differs (beyond 5-second tolerance).

## $REQ_MTS_011: Entry Classification - Resurrection
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A file is classified as Modified (resurrection) when: live on peer, snapshot row exists with `deleted_time IS NOT NULL`. The `deleted_time` is cleared.

## $REQ_MTS_012: Entry Classification - New
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A file is classified as New when: live on peer, no snapshot row exists for this peer.

## $REQ_MTS_013: Entry Classification - Deleted
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A file is classified as Deleted when: absent on peer, snapshot row exists with `deleted_time IS NOT NULL`. The deletion estimate is `deleted_time`.

## $REQ_MTS_014: Entry Classification - Absent-Unconfirmed
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

A file is classified as Absent-unconfirmed when: absent on peer, snapshot row exists with `deleted_time = NULL`.

## $REQ_MTS_015: Entry Classification - Never Existed
**Source:** ./specs/multi-tree-sync.md (Section: "Entry Classification")

When absent on peer and no snapshot row exists, the peer has no opinion (never existed on this peer).

## $REQ_MTS_016: Canon Peer Wins Unconditionally
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

With a canon peer (`+`), the canonical peer's state wins unconditionally: canon has file → push to all others; canon lacks file → delete everywhere else.

## $REQ_MTS_017: Canon Unreachable Exits with Error
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the canon peer is unreachable, the program exits with error at startup.

## $REQ_MTS_018: Rule - All Unchanged
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without a canon peer, if all contributing peers are unchanged, no action is taken.

## $REQ_MTS_019: Rule - Modified Newest Wins
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without a canon peer, if a file is modified, the newest mod_time wins; the file is pushed to all that don't match.

## $REQ_MTS_020: Rule - New File Propagation
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without a canon peer, new files are pushed to all peers that lack them (including peers with no snapshot row). Newest mod_time wins if multiple peers have the file.

## $REQ_MTS_021: Rule - Deleted vs Existing
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Without a canon peer, when a file is deleted on some peers and exists on others: the deletion estimate is compared against the existing file's mod_time. If deletion estimate > mod_time, deletion wins. If mod_time ≥ deletion estimate, the existing file wins.

## $REQ_MTS_022: Rule - Absent-Unconfirmed Resolution
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

For absent-unconfirmed entries: if `last_seen` exceeds the max mod_time of peers that have the entry (by more than 5 seconds), this is a deletion — apply rule 4 using `last_seen` as the deletion estimate. If `last_seen` ≤ max mod_time (or is NULL), this is a failed copy — re-enqueue the copy, no deletion vote.

## $REQ_MTS_023: Rule - Same mod_time Different Size
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

When files have the same mod_time but different sizes, the larger file wins.

## $REQ_MTS_024: Rule - Ties Favor Data
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Ties favor keeping data: existence over deletion, larger over smaller.

## $REQ_MTS_025: No-Row Peers Are Propagation Targets
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Peers with no snapshot row for the entry do not vote — they are simply targets for propagation once a winner is decided.

## $REQ_MTS_026: Timestamp Tolerance 5 Seconds
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

Timestamp tolerance is 5 seconds in either direction. A peer's mod_time within 5 seconds of the maximum is treated as tied. The same tolerance applies to deletion estimate comparisons and absent-unconfirmed resolution.

## $REQ_MTS_048: Skip Copy When Destination Already Matches
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the winning entry already exists on a peer with a matching mod_time (within tolerance) and matching byte_size, no copy is performed for that peer — only the snapshot row is created/updated.

## $REQ_MTS_027: Directory Decisions Are Existence-Based
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

Directories do not use mod_time for decision-making. Directory decisions are existence-based only: if any contributing peer has the directory, it should exist on all peers; if all contributing peers have deleted the directory, delete it on remaining peers.

## $REQ_MTS_028: Canon Overrides Directory Decisions
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

Canon peer overrides directory decisions: canon has it → create everywhere; canon lacks it → delete everywhere.

## $REQ_MTS_029: Directory mod_time Recorded Not Used
**Source:** ./specs/multi-tree-sync.md (Section: "Directory Decisions")

Directory `mod_time` is recorded in the snapshot but not used in decisions.

## $REQ_MTS_030: Type Conflict - File Wins Without Canon
**Source:** ./specs/multi-tree-sync.md (Section: "Type Conflicts")

When the same path is a file on one peer and a directory on another, and no canon peer is designated (or the canon peer has no entry at that path): the file wins. The directory is displaced to BAK/ on the peer(s) that have it, then the winning entry is synced to all peers.

## $REQ_MTS_031: Type Conflict - Canon Type Wins
**Source:** ./specs/multi-tree-sync.md (Section: "Type Conflicts")

When a canon peer is present, its type wins a type conflict.

## $REQ_MTS_032: Snapshot Update - Entry Confirmed Present
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed present on a peer: upsert row with current mod_time, byte_size, `last_seen` set to the current sync timestamp, and `deleted_time = NULL`.

## $REQ_MTS_033: Snapshot Update - Entry Confirmed Absent
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer with an existing row where `deleted_time` is NULL: set `deleted_time` to the row's current `last_seen` value. Do not update `last_seen`.

## $REQ_MTS_034: Snapshot Update - Tombstone Already Set
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer with an existing row where `deleted_time` is already set: no change.

## $REQ_MTS_035: Snapshot Update - Push to Peer
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When a decision pushes to a peer: upsert row for the destination peer with the winning entry's mod_time, byte_size, and `deleted_time = NULL`. Do not update `last_seen` — it is only set when the entry is confirmed present.

## $REQ_MTS_036: Snapshot Update - Copy Completed
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

After a file copy finishes successfully, set `last_seen` to the current sync timestamp on the destination peer's snapshot row. This is the only post-traversal snapshot update.

## $REQ_MTS_037: Snapshot Update - Directory Creation Completed
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

After `create_dir` succeeds on a destination peer, set `last_seen` to the current sync timestamp on that peer's snapshot row.

## $REQ_MTS_038: Snapshot Update - Delete Cascade
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

When deleting from a peer, set `deleted_time` to the row's current `last_seen` on that peer's row, then cascade to descendants using a recursive CTE scoped to the displaced entry's subtree, marking only its descendants.

## $REQ_MTS_039: Inline Displacement
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

All displacement (type conflicts, deletions) executes during the combined-tree walk, not in the operation queue. Displacement is a same-filesystem rename to BAK/.

## $REQ_MTS_040: No Recursion Into Displaced Directory
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

Do not recurse into a directory that is being displaced on a peer. The displacement moves the entire subtree in a single rename, and the snapshot cascade marks all children as deleted.

## $REQ_MTS_041: Snapshot Updated Before File Operations
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

Per-peer snapshot rows are updated during traversal, as soon as a decision is made — before the actual file operations execute.

## $REQ_MTS_042: Incomplete Copy Recovery
**Source:** ./specs/multi-tree-sync.md (Section: "Snapshot Updates")

If the app exits before copies finish, the destination row has `deleted_time = NULL` and `last_seen` unchanged. The next run sees the entry as absent-unconfirmed and re-enqueues the copy via rule 4b.

## $REQ_MTS_043: Offline Peers Excluded
**Source:** ./specs/multi-tree-sync.md (Section: "Offline Peers")

Unreachable peers are excluded entirely — they do not participate in listings or decisions. Their snapshot rows are not modified. On the next run when reachable, discrepancies between their filesystem state and snapshot rows drive sync decisions.

## $REQ_MTS_044: Canon Required on First Sync
**Source:** ./specs/sync.md (Section: "Canon Peer (+)")

Canon is required when no peer in the group has snapshot history (first run). Without snapshots, there's no history to distinguish new from deleted, so one peer must be the source of truth.

## $REQ_MTS_045: Bidirectional After First Sync
**Source:** ./specs/sync.md (Section: "Canon Peer (+)")

Once snapshots exist, bidirectional sync works without a canon peer.

## $REQ_MTS_046: Multiple Deletion Estimates Use Most Recent
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If multiple peers have deleted an entry, the most recent deletion estimate among the deleting peers is used for comparison against the existing file's mod_time.

## $REQ_MTS_047: Case Sensitivity Preserved
**Source:** ./specs/sync.md (Section: "Case Sensitivity")

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive and case-insensitive filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BAK/.
