# Cleanup

BAK/ and TMP/ directory cleanup and tombstone purging.

## $REQ_CLEAN_001: BAK Cleanup by Age
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

At each directory level during the walk, expired BAK/ entries are cleaned up when `--bd > 0`. Age is determined from the timestamp directory name, not filesystem modification time.

## $REQ_CLEAN_002: TMP Cleanup by Age
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

At each directory level during the walk, expired TMP/ entries are cleaned up when `--xd > 0`. Age is determined from the timestamp directory name, not filesystem modification time.

## $REQ_CLEAN_003: BAK Cleanup Disabled with Zero
**Source:** ./README.md (Section: "Global Options")

Setting `--bd 0` disables BAK/ cleanup (displaced files kept forever).

## $REQ_CLEAN_004: TMP Cleanup Disabled with Zero
**Source:** ./README.md (Section: "Global Options")

Setting `--xd 0` disables TMP/ cleanup (stale staging kept forever).

## $REQ_CLEAN_005: Tombstone Purge by Age
**Source:** ./specs/algorithm.md (Section: "Startup")

At startup, when `--td > 0`, tombstone rows (where `deleted_time IS NOT NULL`) older than `--td` days are deleted from each peer's snapshot.

## $REQ_CLEAN_006: Stale Row Purge
**Source:** ./specs/algorithm.md (Section: "Startup")

At startup, when `--td > 0`, non-tombstone rows with `last_seen IS NOT NULL` older than `--td` days are also purged. Rows with `last_seen = NULL` (pending copies) are not purged.

## $REQ_CLEAN_007: Tombstone Purge Disabled with Zero
**Source:** ./specs/algorithm.md (Section: "Startup")

Setting `--td 0` skips tombstone and stale row purging entirely.

## $REQ_CLEAN_008: Snapshot Upload Failure Leaves TMP
**Source:** ./specs/algorithm.md (Section: "Errors")

If snapshot upload fails, the TMP staging file is left in place for eventual `--xd` cleanup.

## $REQ_CLEAN_009: Cleanup Removes Entire Timestamp Directory
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

Cleanup deletes entire timestamp directories (and all contents) when the timestamp is older than the threshold, including nested UUID directories for TMP.
