# Sync Algorithm

Core sync walk, entry classification, decision rules, directory decisions, type conflicts, and snapshot updates.

## $REQ_SYNC_001: Pre-Order Traversal
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

The traversal is pre-order: every entry in a directory is decided and acted on before recursing into any subdirectory. A directory marked for displacement is renamed (with its entire subtree) before its children are ever visited.

## $REQ_SYNC_002: Parallel Directory Listing
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

At each directory level, all peers are listed in parallel. The union of entry names across peers forms the set of entries to process.

## $REQ_SYNC_003: Listing Error Excludes Peer
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

If a peer's listing fails at a directory, that peer is excluded from the entire subtree below that directory.

## $REQ_SYNC_004: Entry Classification - Unchanged
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

A file that is live on a peer with the same mod_time (within 5-second tolerance) as its snapshot row is classified as Unchanged.

## $REQ_SYNC_005: Entry Classification - Modified
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

A file that is live on a peer with a different mod_time from its snapshot row is classified as Modified.

## $REQ_SYNC_006: Entry Classification - New
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

A file that is live on a peer with no snapshot row is classified as New.

## $REQ_SYNC_007: Entry Classification - Resurrection
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

A file that is live on a peer where the snapshot row has `deleted_time` set (tombstone) is classified as Resurrection. The tombstone is cleared.

## $REQ_SYNC_008: Entry Classification - Deleted
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

A file that is absent on a peer where the snapshot row has `deleted_time IS NOT NULL` is classified as Deleted.

## $REQ_SYNC_009: Entry Classification - Absent Unconfirmed
**Source:** ./specs/algorithm.md (Section: "Entry Classification")

A file that is absent on a peer where the snapshot row has `deleted_time IS NULL` is classified as Absent-unconfirmed.

## $REQ_SYNC_010: Five Second Mod-Time Tolerance
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

A 5-second tolerance applies to mod_time comparisons: classification (mod_time vs snapshot), decision comparisons (mod_time vs mod_time, deletion estimate vs mod_time), and absent-unconfirmed rule 4b (last_seen vs max mod_time).

## $REQ_SYNC_011: Canon Peer Wins Unconditionally
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

When a canon peer (`+`) is present: if canon has the file, push to all others; if canon lacks the file, delete everywhere else (displace to BAK/).

## $REQ_SYNC_012: Newest Mod-Time Wins
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

Without a canon peer, among contributing peers, the file with the newest mod_time wins and is pushed to peers that differ.

## $REQ_SYNC_013: Tie-Break by File Size
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

When multiple contributing peers have mod_times within 5-second tolerance, the larger file wins.

## $REQ_SYNC_014: All Peers Agree No Copy
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

If all contributing peers agree on mod_time (within tolerance) and byte_size, no copy is needed.

## $REQ_SYNC_015: Deletion vs Existence
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

When some contributing peers have the file live and others have it deleted, the deletion estimate is compared to the live mod_time. If the deletion estimate exceeds the live mod_time by more than 5 seconds, delete everywhere. Otherwise, the existing file wins (ties favor existence).

## $REQ_SYNC_016: Absent-Unconfirmed Handling
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

For absent-unconfirmed entries: if `last_seen` is NULL, the entry is treated as a pending copy that never completed and is re-enqueued. If `last_seen` exceeds all live mod_times by more than 5 seconds, it is treated as a confirmed deletion. Otherwise, the entry is treated as needing the file.

## $REQ_SYNC_030: Absent-Unconfirmed No Physical File Deletes
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

If all contributing voters for an entry are absent-unconfirmed and no peer physically has the file, the entry is treated as deleted rather than re-enqueued.

## $REQ_SYNC_017: All Deleted Means Delete
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

If all contributing voters have the entry deleted, it is deleted on all peers.

## $REQ_SYNC_018: All Unchanged No Action
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

If all contributing voters classify the entry as Unchanged, no action is taken.

## $REQ_SYNC_019: Directory Decisions Existence-Based
**Source:** ./specs/algorithm.md (Section: "Directory Decisions")

Directory decisions do not use mod_time. If any contributing peer has the directory, it is created on peers that lack it. If all contributing peers have it deleted (tombstone + absent), it is deleted everywhere.

## $REQ_SYNC_020: Type Conflict No Canon
**Source:** ./specs/algorithm.md (Section: "Type Conflicts")

When the same path is a file on one peer and a directory on another, and no canon peer is present, the file wins. The directory is displaced to BAK/, then the file is synced normally.

## $REQ_SYNC_021: Type Conflict With Canon
**Source:** ./specs/algorithm.md (Section: "Type Conflicts")

When the same path has a type conflict and a canon peer is present, canon's type wins.

## $REQ_SYNC_022: Snapshot Updated Before Copies
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

Snapshots are updated during traversal as soon as a decision is made — before file copies execute. The snapshot reflects the decided state, not the physical state.

## $REQ_SYNC_023: Snapshot Entry Present
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When an entry is confirmed present on a peer, the snapshot row is upserted with `mod_time`, `byte_size`, `last_seen = now`, and `deleted_time = NULL`.

## $REQ_SYNC_024: Snapshot Push Target
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When a decision is to push to a peer, the snapshot row is upserted with the winner's `mod_time` and `byte_size`, `deleted_time = NULL`, but `last_seen` is NOT set — it is only set after the copy completes or after listing confirms presence.

## $REQ_SYNC_025: Snapshot Delete Cascade
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When a directory is deleted from a peer, `deleted_time` is set on the directory's snapshot row and recursively cascaded to all descendant rows that have `deleted_time IS NULL`.

## $REQ_SYNC_026: Skip Unnecessary Copies
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

If the winning entry already exists on a peer with matching mod_time (within tolerance) and matching byte_size, no copy is performed — only the snapshot row is updated.

## $REQ_SYNC_027: No Opinion Entries Skipped
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

Peers with no snapshot row and absent state have no opinion and are skipped in voting.

## $REQ_SYNC_028: No Contributing Voters Deletes Subordinates
**Source:** ./specs/algorithm.md (Section: "Decision Rules")

When no contributing peer has the entry (all have no opinion), subordinate peers that have it get displaced.

## $REQ_SYNC_029: Snapshot Upload Atomic
**Source:** ./specs/algorithm.md (Section: "Startup")

Updated snapshots are uploaded back to each peer via TMP staging with atomic rename.

## $REQ_SYNC_031: Snapshot Tombstone Creation
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When an entry is confirmed absent on a peer and its snapshot row has `deleted_time IS NULL`, the snapshot row is updated with `deleted_time = last_seen`.

## $REQ_SYNC_032: Snapshot Post-Copy Update
**Source:** ./specs/algorithm.md (Section: "Snapshot Updates")

When a file copy completes successfully, `last_seen` is set to now on the destination peer's snapshot row.

## $REQ_SYNC_033: Dry-Run Skips Mutations
**Source:** ./specs/algorithm.md (Section: "Dry Run Mode")

In dry-run mode (`--dry-run` or `-n`), all mutating operations are skipped: file copies, displacements to BAK/, directory creation/deletion, snapshot uploads, and BAK/TMP cleanup. Connections, snapshot downloads, directory walks, decisions, and logging still occur.

## $REQ_SYNC_034: Dry-Run Logs Operations
**Source:** ./specs/algorithm.md (Section: "Dry Run Mode")

In dry-run mode, `C <path>` and `X <path>` are logged for every operation that would happen, identical to a real run.

## $REQ_SYNC_035: Case Collision Warning
**Source:** ./specs/algorithm.md (Section: "Case Sensitivity")

When syncing to a case-insensitive filesystem with multiple files differing only in case, a warning is logged when case collision is detected. The last file encountered (lexicographic order) overwrites earlier ones.

## $REQ_SYNC_036: Filename Byte Comparison
**Source:** ./specs/algorithm.md (Section: "Unicode Normalization")

Filenames are compared byte-for-byte as reported by the filesystem. No Unicode normalization is performed.

## $REQ_SYNC_037: Copy and Delete Logging
**Source:** ./specs/algorithm.md (Section: "Logging")

Every file copy is logged as `C <relative-path>` and every deletion as `X <relative-path>` at info level. Logged once per decision, not per peer.

## $REQ_SYNC_038: All Output to Stdout
**Source:** ./specs/algorithm.md (Section: "Logging")

All output goes to stdout. No output to stderr.
