# 010_file-transfer-safety: File transfer replacement safety

## Behavior
This concern derives from `specs/sync.md` sections "Rename Compatibility", "File Copy", and transfer-related parts of "Errors". It covers bounded file-content streaming, local copy optimization boundaries, SWAP `new` and `old` sequencing for user-file replacement, modification-time setting after copy, cleanup after failed copy phases, and failure behavior before and after SWAP `old` exists.

## $REQ_IDs
- `010.1` -- KitchenSync writes each copied file's replacement content to the destination peer's SWAP `new` path before moving the content to the final path.
- `010.2` -- When a copied file replaces an existing destination file, KitchenSync moves the existing destination file to the destination peer's SWAP `old` path before moving SWAP `new` to the final path.
- `010.3` -- KitchenSync replaces an existing destination file on transports whose rename operation rejects overwriting an existing destination path.
- `010.4` -- After SWAP `new` is moved to the final path, KitchenSync sets the destination file's modification time to the winning modification time from the sync decision.
- `010.5` -- KitchenSync sets the copied destination file's modification time without re-reading the source file's modification time after the copy.
- `010.6` -- After a replacement copy has moved SWAP `new` to the final path, KitchenSync moves SWAP `old` to BAK when SWAP `old` exists.
- `010.7` -- KitchenSync cleans up empty SWAP directories after file-copy replacement work.
- `010.8` -- KitchenSync streams file contents with bounded buffering whose total buffer size is independent of the file size.
- `010.9` -- KitchenSync begins writing streamed file contents before the entire source file is buffered in memory.
- `010.10` -- Local `file://` to `file://` copies produce the same SWAP `new`, SWAP `old`, final-path replacement, modification-time, BAK archive, and failure-cleanup outcomes as other file transfers.
- `010.11` -- If moving an existing destination file to SWAP `old` fails, the original destination file remains in place.
- `010.12` -- If moving an existing destination file to SWAP `old` fails, KitchenSync cleans up staged files for that transfer when possible.
- `010.13` -- If moving an existing destination file to SWAP `old` fails, KitchenSync skips that copy for the run.
- `010.14` -- If a transfer fails before SWAP `old` exists, KitchenSync removes the SWAP `new` file or directory for that transfer when possible before ending the copy attempt.
- `010.15` -- If KitchenSync gives up on a transfer failure before SWAP `old` exists, it logs a final failure for that copy.
- `010.16` -- If KitchenSync gives up on a transfer failure before SWAP `old` exists, it skips that file for the run.
- `010.17` -- If a transfer fails after SWAP `old` exists, KitchenSync leaves the SWAP state in place.
- `010.18` -- If a transfer fails after SWAP `old` exists, KitchenSync logs an error for the transfer failure.
- `010.19` -- If archiving SWAP `old` to BAK fails after the replacement is in place, KitchenSync leaves SWAP `old` in place for later recovery.
- `010.20` -- If archiving SWAP `old` to BAK fails after the replacement is in place, KitchenSync logs an error.
- `010.21` -- TMP or SWAP staging failure is handled as a transfer failure.
- `010.22` -- If setting the destination modification time fails after a completed copy, KitchenSync leaves the copied file in place.
- `010.23` -- If setting the destination modification time fails after a completed copy, KitchenSync logs an error.

## Notes
This category owns the mechanics of executing an individual file transfer safely once copy work is selected. Copy queue scheduling, copy-slot limits, and retry scheduling belong to `013_concurrency-controls`; snapshot replacement SWAP belongs to `006_snapshot-lifecycle`; general SWAP recovery and BAK/TMP maintenance outside a running copy belong to `011_displacement-and-staging-cleanup`.
