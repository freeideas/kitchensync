# Cleanup

Retention policies and purging of expired data.

## $REQ_CLEAN_001: Cleanup Runs Before Traversal
**Source:** ./specs/sync.md (Section: "Run")

Expired tombstones, log entries, stale XFER directories, and BACK directories are purged at the start of a sync run, before traversal begins.

## $REQ_CLEAN_002: Tombstone Purge
**Source:** ./specs/database.md (Section: "Tombstones")

Tombstones (snapshot rows with `del_time` set) are purged after `tombstone-retention-days` (default: 180 days).

## $REQ_CLEAN_003: Log Entry Purge
**Source:** ./README.md (Section: "Cleanup"), ./specs/quartz-lifecycle.md (Section: "Logging")

Log entries are purged after `log-retention-days` (default: 32 days).

## $REQ_CLEAN_006: Log Purge on Every Insert
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

On every log insert, entries older than `log-retention-days` are purged.

## $REQ_CLEAN_004: XFER Directory Purge
**Source:** ./specs/sync.md (Section: "XFER Staging")

Stale `.kitchensync/XFER/` directories are purged after `xfer-cleanup-days` (default: 2 days).

## $REQ_CLEAN_005: BACK Directory Purge
**Source:** ./specs/sync.md (Section: "BACK Directory")

`.kitchensync/BACK/` directories are purged after `back-retention-days` (default: 90 days).
