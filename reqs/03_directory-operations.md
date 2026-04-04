# Directory Operations

Directory decisions, type conflicts, and cascade tombstones.

## $REQ_DIR_001: Directory Existence-Based Decisions
**Source:** ./specs/algorithm.md (Section: "Directory Decisions")

Directories do not use mod_time for decisions. If any contributing peer has the directory, it is created on peers that lack it. If all contributing peers have deleted it, it is deleted everywhere.

## $REQ_DIR_002: Empty Directories Preserved
**Source:** ./specs/algorithm.md (Section: "Directory Decisions")

A directory that exists but is empty remains -- there is no automatic cleanup of empty directories.

## $REQ_DIR_003: Canon Overrides for Directories
**Source:** ./specs/algorithm.md (Section: "Directory Decisions")

Canon overrides apply to directories as usual -- canon's state wins unconditionally.

## $REQ_DIR_004: Type Conflict File Wins
**Source:** ./specs/algorithm.md (Section: "Type Conflicts")

When the same path is a file on one peer and a directory on another, and no canon is present, the file wins. The directory is displaced to BAK/, then the file is synced normally.

## $REQ_DIR_005: Type Conflict Canon Wins
**Source:** ./specs/algorithm.md (Section: "Type Conflicts")

When there is a type conflict and a canon peer is present, the canon peer's type wins.

## $REQ_DIR_006: Cascade Tombstones on Directory Displacement
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

When a directory is displaced (deleted), all descendant snapshot rows are updated with `deleted_time` set. The `deleted_time` is the displaced entry's own `last_seen` value. The cascade MUST traverse through descendant rows that already have `deleted_time` set (do not filter on `deleted_time IS NULL` during traversal).

## $REQ_DIR_007: Directory Creation is Inline
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

Directory creation and displacement run inline during the walk (not queued like file copies). After creation, `last_seen` is set to now on the peer's snapshot row.

## $REQ_DIR_008: Wrong Type Displaced Before Action
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

If a peer has the wrong type at an entry path (file where directory expected, or vice versa), the wrong-type entry is displaced to BAK/ before the correct action proceeds.
