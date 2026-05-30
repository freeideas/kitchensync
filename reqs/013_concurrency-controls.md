# 013_concurrency-controls: Concurrency limits and scheduling

## Behavior
This concern derives from `specs/concurrency.md` sections "Copy Concurrency" and "Copy Queue Tries", plus `specs/sync.md` sections "Run" and "Operation Queue". It covers the global `--max-copies` active file-copy limit, the distinction between copy concurrency and other concurrent work, incremental copy scheduling while traversal continues, per-copy try counting and retry scheduling, and the absence of user-facing per-peer, per-host, or per-connection transfer limits.

## $REQ_IDs
- `013.1` -- By default, at most 10 file-copy operations are active at one time across the whole run.
- `013.2` -- `--max-copies N` sets the maximum number of active file-copy operations across the whole run to `N`.
- `013.3` -- Each active file copy counts as one active file-copy operation regardless of whether the source and destination peers use `file://`, `sftp://`, or mixed schemes.
- `013.4` -- Directory listing, snapshot download, snapshot upload, directory creation, BAK cleanup, TMP cleanup, and SWAP cleanup do not count as active file-copy operations.
- `013.5` -- Non-copy work running concurrently with file copies does not allow more than `--max-copies` active file-copy operations.
- `013.6` -- The CLI exposes no per-peer, per-host, or per-connection transfer-limit setting.
- `013.7` -- File-copy work begins before the full tree scan has completed when traversal finds eligible copy work in an early directory.
- `013.8` -- File-copy work found during the combined-tree walk is enqueued for concurrent execution subject to the global active-copy limit.
- `013.9` -- A run waits for all enqueued file-copy work to complete before reporting successful completion and exiting 0.
- `013.10` -- Directory creation and displacement to BAK run inline during the combined-tree walk rather than as queued file-copy work.
- `013.11` -- Each queued file copy has an independent try count.
- `013.12` -- `--retries-copy N` sets the maximum total tries for each queued file copy to `N`, including the first try.
- `013.13` -- Each failed file-copy try counts against that queued file copy's `--retries-copy` total-try limit.
- `013.14` -- When a file-copy try fails before reaching the `--retries-copy` total-try limit, that file copy is moved to the back of the queue for retry.
- `013.15` -- When a file-copy try fails at the `--retries-copy` total-try limit, that file copy is not requeued for another try during the run.
- `013.16` -- Copy try limits apply the same way to local copies, SFTP copies, and mixed-scheme copies.

## Notes
This category owns copy scheduling and concurrency limits. Directory-listing concurrency, listing visibility, and listing failure consequences belong to `007_traversal-and-excludes`; transfer failure phase behavior belongs to `010_file-transfer-safety`; user-visible progress and trace output belong to `014_logging-and-progress`.
