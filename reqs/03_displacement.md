# Displacement to BACK

Moving displaced files and directories to `.kitchensync/BACK/` for recovery.

## $REQ_BACK_001: Displacement Path
**Source:** ./specs/sync.md (Section: "Displace to BACK")

Displaced entries are renamed to `<parent>/.kitchensync/BACK/<timestamp>/<basename>`.

## $REQ_BACK_002: Directory Displacement as Single Rename
**Source:** ./specs/sync.md (Section: "Displace to BACK")

A displaced directory is moved as a single rename, preserving its entire subtree.

## $REQ_BACK_003: Never Destructive
**Source:** ./README.md (Section: "Why KitchenSync?")

No file is ever destroyed. Old copies of overwritten or deleted files go to `.kitchensync/BACK/`.

