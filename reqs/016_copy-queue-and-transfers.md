# 016_copy-queue-and-transfers: Copy queue, concurrency, and file transfer execution

## Behavior
This concern derives from `specs/sync.md` sections "Operation Queue" and "File
Copy", `specs/concurrency.md` sections "Copy Concurrency" and "Copy Queue
Tries", `plan/sftp-client.md`, `plan/local-fs-ops.md`, and
`plan/local-file-metadata.md`. It covers incremental queuing of copy work, the
global active-copy limit, copy-slot accounting across schemes, per-copy try
counts and retry ordering, bounded-buffer streaming, local-copy optimization
limits, the observable file replacement sequence used by queued transfers, and
transfer failure handling before and after durable replacement state exists.

## $REQ_IDs
- `016.1` -- KitchenSync starts eligible file-copy work before the whole tree has been scanned.
- `016.2` -- Without `--max-copies`, KitchenSync runs at most 10 active file copies at the same time.
- `016.3` -- With `--max-copies N`, KitchenSync runs at most `N` active file copies at the same time.
- `016.4` -- The active file-copy limit applies across the whole run rather than separately per peer.
- `016.5` -- `file://` to `file://` copies count against the active file-copy limit.
- `016.6` -- `file://` to `sftp://` copies count against the active file-copy limit.
- `016.7` -- `sftp://` to `file://` copies count against the active file-copy limit.
- `016.8` -- `sftp://` to `sftp://` copies count against the active file-copy limit.
- `016.9` -- Directory listing does not count against the active file-copy limit.
- `016.10` -- Snapshot download and upload do not count against the active file-copy limit.
- `016.11` -- Directory creation does not count against the active file-copy limit.
- `016.12` -- BAK, TMP, and SWAP cleanup do not count against the active file-copy limit.
- `016.13` -- KitchenSync imposes no per-peer active-copy limit lower than the global active file-copy limit.
- `016.14` -- KitchenSync imposes no per-host active-copy limit lower than the global active file-copy limit.
- `016.15` -- KitchenSync imposes no per-connection active-copy limit lower than the global active file-copy limit.
- `016.16` -- Failed tries for one queued file copy do not consume tries for any other queued file copy.
- `016.17` -- Without `--retries-copy`, KitchenSync allows at most 3 total tries for each queued file copy, including the first try.
- `016.18` -- `--retries-copy N` allows at most `N` total tries for each queued file copy, including the first try.
- `016.19` -- After a copy try fails before reaching its `--retries-copy` total-try limit, KitchenSync moves that queued copy behind other queued copy work.
- `016.20` -- After a copy try fails before reaching its `--retries-copy` total-try limit, other queued copy work continues in the same run.
- `016.21` -- After a queued copy reaches its `--retries-copy` total-try limit, KitchenSync does not requeue that copy again in the same run.
- `016.22` -- Copy try limits apply the same way to local copies, SFTP copies, and mixed-scheme copies.
- `016.23` -- Each transfer writes replacement content to `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new` before replacing the final destination path.
- `016.24` -- When the destination already has a file at the target path, KitchenSync moves that existing file to `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old` before moving SWAP `new` into the final path.
- `016.25` -- KitchenSync moves SWAP `new` into the final destination path after any existing destination file has been moved to SWAP `old`.
- `016.26` -- After moving SWAP `new` into the final destination path, KitchenSync sets the destination file modification time to the winning modification time from the sync decision.
- `016.27` -- When SWAP `old` exists after SWAP `new` has been moved into the final destination path, KitchenSync archives SWAP `old` to `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>`.
- `016.28` -- A copy to a destination path that had no existing file creates no BAK entry for that destination path.
- `016.29` -- After a successful transfer, KitchenSync removes the empty SWAP directories for that transfer.
- `016.30` -- When moving an existing destination file to SWAP `old` fails, the original destination remains in place.
- `016.31` -- When moving an existing destination file to SWAP `old` fails and staged files can be removed, KitchenSync removes the staged files for that transfer.
- `016.32` -- When moving an existing destination file to SWAP `old` fails, KitchenSync skips that copy for the rest of the run.
- `016.33` -- When a transfer fails before the existing destination has been moved to SWAP `old`, KitchenSync deletes that transfer's SWAP `new` entry before releasing the copy slot.
- `016.34` -- When a transfer fails after the existing destination has been moved to SWAP `old`, KitchenSync leaves the transfer's SWAP state in place.
- `016.35` -- Active transfers stream file content without buffering the entire file in memory before destination writing begins.
- `016.36` -- The total buffer memory used by an active transfer is independent of the copied file size.
- `016.37` -- A local-to-local file copy does not write replacement content directly to the final destination path.

## Notes
This file covers queued file copy work. Directory creation and displacement are
inline traversal actions, staging directory recovery belongs to
`017_staging-recovery-and-cleanup.md`, and dry-run deviations belong to
`018_dry-run.md`.
