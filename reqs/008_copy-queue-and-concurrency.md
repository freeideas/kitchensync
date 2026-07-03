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

## Notes
This category owns scheduling, limits, retries, and transfer attempts.
Filesystem layout for SWAP, BAK, and TMP belongs to `009_recoverable-staging`.

## $REQ_IDs
