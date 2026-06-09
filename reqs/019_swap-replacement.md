# 019_swap-replacement: SWAP-based safe file replacement and recovery

## Behavior
This concern derives from `specs/sync.md` sections "Rename Compatibility" (the
user-data paragraph), "File Copy" (the SWAP step sequence), "SWAP Directory",
and the transfer-failure error rows of "Errors"; plus
`specs/multi-tree-sync.md` section "SWAP Recovery During Traversal".

It covers how a file copy that would replace an existing destination is made
recoverable without relying on rename-over-existing (which SFTP may reject). The
ordered replacement: write the new content to SWAP `new`, move any existing
destination to SWAP `old`, rename `new` into the final path, set the winning
mod_time, archive `old` to BAK, then clean up empty SWAP directories. It covers
the SWAP path layout under `<parent>/.kitchensync/SWAP/<encoded-basename>/` with
`new` and `old`, the percent-encoded basename segment, and the requirement to
recover or fail any existing SWAP for a basename before starting a replacement.
It covers the five traversal-time SWAP recovery states (combinations of `old`,
`new`, and target presence) and the failure handling: a failure before `old`
exists deletes staged `new` and requeues or fails the copy by try count; a
failure after `old` exists leaves SWAP state in place for later recovery; an
archive-old failure leaves `old` for later recovery; a failed SWAP recovery
treats the peer's listing for that directory as failed.

The snapshot-database form of the same SWAP discipline is `016_snapshot-storage`.
The copy-slot, retry-count, streaming, and native-copy mechanics are
`020_copy-execution`. The BAK destination layout and cleanup are
`021_staging-and-displacement`. Setting the winning mod_time is part of this
sequence; the snapshot row it corresponds to is `017_snapshot-updates`.

## $REQ_IDs

- `019.1` -- A file copy that replaces an existing destination writes the new content to the SWAP `new` path `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new` before replacing the target.
- `019.2` -- When the destination already holds a file at the target path, the copy moves that existing file to the SWAP `old` path `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old` before swapping in the new content.
- `019.3` -- The copy renames the SWAP `new` file to the final target path.
- `019.4` -- After the new file is in place, the copy sets the destination file's modification time to the winning mod_time from the decision rather than a time re-read from the source.
- `019.5` -- When SWAP `old` exists after the new file is in place, the copy archives it to BAK.
- `019.6` -- The copy removes empty SWAP directories after the replacement completes.
- `019.7` -- The `<encoded-basename>` SWAP path segment is the target basename percent-encoded so it forms a single path segment on the transport.
- `019.8` -- Before starting a replacement for a path, KitchenSync recovers or fails any existing SWAP directory for that basename.
- `019.9` -- On transfer failure before SWAP `old` exists, the staged SWAP `new` file or directory for that transfer is deleted.
- `019.10` -- When moving the existing destination to SWAP `old` fails, the original destination remains in place.
- `019.11` -- When moving the existing destination to SWAP `old` fails, the copy is skipped for that run.
- `019.12` -- On transfer failure after SWAP `old` exists, the SWAP state is left in place for later recovery.
- `019.13` -- When archiving SWAP `old` to BAK fails after the replacement is in place, SWAP `old` is left in place for later recovery.
- `019.14` -- In a normal run, KitchenSync recovers each `.kitchensync/SWAP/<encoded-basename>` directory before that directory's live entries are listed for sync decisions.
- `019.15` -- During SWAP recovery, when `old` exists and the target exists, KitchenSync moves `old` to BAK and removes the now-empty SWAP directory.
- `019.16` -- During SWAP recovery, when `old` exists, `new` exists, and the target is missing, KitchenSync renames `new` to the target, moves `old` to BAK, and removes the now-empty SWAP directory.
- `019.17` -- During SWAP recovery, when `old` exists, `new` is missing, and the target is missing, KitchenSync renames `old` back to the target and removes the now-empty SWAP directory.
- `019.18` -- During SWAP recovery, when `old` is missing, `new` exists, and the target exists, KitchenSync deletes `new` and removes the now-empty SWAP directory.
- `019.19` -- During SWAP recovery, when `old` is missing, `new` exists, and the target is missing, KitchenSync renames `new` to the target and removes the now-empty SWAP directory.
- `019.20` -- When recovery of a SWAP directory fails, KitchenSync treats that peer's listing for the current directory as failed and excludes the peer from that directory subtree.
- `019.21` -- In `--dry-run`, peer-side SWAP recovery during traversal is skipped.

## Notes

The plan's prose mentions the requeue-or-fail-by-try-count decision and copy-slot
release on transfer failure; those retry-count and slot mechanics belong to
`020_copy-execution` and are not bulleted here. This file keeps only the
SWAP-specific cleanup (deleting staged `new`). The BAK timestamped path layout is
owned by `021_staging-and-displacement`; bullets here reference BAK only as the
archive destination.
