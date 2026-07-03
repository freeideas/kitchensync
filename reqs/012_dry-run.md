# 012_dry-run: Dry-run behavior

## Behavior
This concern derives from `specs/sync.md` sections "Startup", "Run", "Dry
Run", "Operation Queue", "File Copy", and "Errors", `specs/database.md`
section "Database", `specs/multi-tree-sync.md` sections "SWAP Recovery During
Traversal" and "BAK/TMP Cleanup During Traversal", and `specs/SCENARIOS.md`
scenario S-08 and property "P-05: Dry Run Does Not Write Peer State". It covers
the observable read-only behavior of `--dry-run`: connecting only to existing
peer roots, downloading and locally updating temporary snapshots, listing and
planning normally, exercising copy slots and source reads, printing the dry-run
line, suppressing peer writes, skipping peer-side SWAP recovery and BAK/TMP
cleanup, and not uploading snapshots.

## Notes
This category owns the cross-cutting no-peer-write guarantee for dry runs.
The normal behavior being simulated remains owned by each operation category.

## $REQ_IDs
