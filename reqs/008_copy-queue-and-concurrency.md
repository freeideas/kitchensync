# 008_copy-queue-and-concurrency: Copy queue and concurrency

## Behavior
This concern derives from `specs/concurrency.md` sections "Copy Concurrency",
"Directory Listing", "Copy Queue Tries", and "Trace Logging", and
`specs/sync.md` sections "Run", "Operation Queue", "Dry Run", "File Copy",
"Errors", and "Peer Transports". It covers the observable global active-copy
limit, incremental copy start while traversal continues, queued copy try
counts, retry and give-up behavior, copy-slot accounting across all peer
schemes, bounded streaming behavior, local-copy equivalence to streamed copy,
and trace output for copy-slot acquire and release events.

## $REQ_IDs
- `008.1` -- Without `--max-copies`, no more than 10 file-copy transfers hold active copy slots at the same time across the whole run.
- `008.2` -- With `--max-copies N`, no more than `N` file-copy transfers hold active copy slots at the same time across the whole run.
- `008.3` -- The active-copy limit applies across all source and destination peer scheme combinations.
- `008.4` -- KitchenSync does not impose an observable per-peer, per-host, or per-connection copy limit below the global active-copy limit.
- `008.5` -- Directory listing, snapshot download, snapshot upload, directory creation, BAK cleanup, TMP cleanup, and SWAP cleanup do not consume active copy slots.
- `008.6` -- At a directory level, KitchenSync starts listing operations for every reachable peer before waiting for any listing result from that directory level.
- `008.7` -- When an early scanned directory produces eligible copy work and later directories remain unscanned, KitchenSync does not wait for the whole tree scan before starting that copy work.
- `008.8` -- Opportunistic old snapshot-row cleanup does not delay the first directory scan.
- `008.9` -- Opportunistic old snapshot-row cleanup does not delay the first eligible file copy.
- `008.10` -- Each destination copy is executed as a separate transfer from one source peer path to one destination peer path.
- `008.11` -- Each queued copy tracks its copy tries independently from other queued copies.
- `008.12` -- `--retries-copy N` gives each queued copy at most `N` total tries, including the first try.
- `008.13` -- When a copy try fails before the existing destination is moved aside and the queued copy has remaining tries, KitchenSync moves that queued copy behind other queued work.
- `008.14` -- When a copy try fails before the existing destination is moved aside and the queued copy has reached its total try limit, KitchenSync does not try that queued copy again in the same run.
- `008.15` -- Copy try limits apply the same way to local-to-local, local-to-SFTP, SFTP-to-local, and SFTP-to-SFTP transfers.
- `008.16` -- A transfer acquires one global copy slot before starting the transfer.
- `008.17` -- Every successful transfer, including a local-to-local transfer, writes the selected source file bytes to the destination file.
- `008.18` -- Every successful transfer, including a local-to-local transfer, sets the destination file modification time to the winning modification time selected for that file.
- `008.19` -- If setting the destination modification time fails after a completed copy, KitchenSync does not undo the copied destination file.
- `008.20` -- File content is transferred with bounded buffering whose total active buffer size is independent of the file size.
- `008.21` -- KitchenSync waits for all enqueued file copies to complete before uploading updated snapshots to peers.
- `008.22` -- In `--dry-run`, queued copy work acquires active copy slots.
- `008.23` -- In `--dry-run`, queued copy work reads source files.
- `008.24` -- In `--dry-run`, queued copy work applies the same copy try-limit behavior as a normal run.
- `008.25` -- At `trace` verbosity, KitchenSync emits a `copy-slots active=<n>/<max>` line when a transfer acquires a copy slot.
- `008.26` -- At `trace` verbosity, KitchenSync emits a `copy-slots active=<n>/<max>` line when a transfer releases a copy slot.
- `008.27` -- Copy-slot trace lines report global active file-copy slots, not network connections.

## Notes
This category owns scheduling, limits, retries, and transfer attempts.
Filesystem layout for SWAP, BAK, and TMP belongs to `009_recoverable-staging`.
