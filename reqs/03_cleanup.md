# Cleanup and Retention

Default retention policies for BACK/, XFER/, tombstones, and log entries.

## $REQ_CLEAN_001: Log Entry Retention
**Source:** ./README.md (Section: "Cleanup")

Log entries are retained for 32 days by default (`log-retention-days`).

## $REQ_CLEAN_002: XFER Directory Retention
**Source:** ./README.md (Section: "Cleanup")

`.kitchensync/XFER/` directories are retained for 2 days by default (`xfer-cleanup-days`).

## $REQ_CLEAN_003: BACK Directory Retention
**Source:** ./README.md (Section: "Cleanup")

`BACK/` directories are retained for 90 days by default (`back-retention-days`).

## $REQ_CLEAN_004: Tombstone Retention
**Source:** ./README.md (Section: "Cleanup")

Tombstones are retained for 180 days by default (`tombstone-retention-days`).
