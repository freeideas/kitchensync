# 020_copy-execution: Copy concurrency, retries, and transfer mechanics

## Behavior
This concern derives from `specs/concurrency.md` sections "Copy Concurrency",
"Directory Listing", and "Copy Queue Tries", plus `specs/sync.md` section "File
Copy" (the streaming and native-copy paragraphs) and "Operation Queue".

It covers the global active-copy limit: at most `--max-copies` file copies hold
slots at once across the whole run regardless of scheme, while listing, snapshot
transfer, directory creation, and BAK/TMP/SWAP cleanup do not count and may run
concurrently. It covers incremental copying (no whole-tree pre-scan; early copy
work occupies slots while later directories are still being scanned) and the
concurrent issuing of all reachable peers' directory listings at each level. It
covers per-copy try counting: `--retries-copy` is the maximum total tries
including the first, a failed try under the limit moves the copy to the back of
the queue and other work continues, and a copy that reaches the limit is marked
failed for the run; try limits apply identically to local, SFTP, and
mixed-scheme copies. It covers transfer mechanics: streaming with bounded,
file-size-independent buffering (never buffering the whole file before writing),
and the option to use the host filesystem's native copy primitive when both ends
are local while preserving the same SWAP safety boundary.

The SWAP step sequence, mod_time setting, and per-transfer failure recovery are
`019_swap-replacement`. The `C`/`X` progress lines and `copy-slots` trace output
are `023_logging`. Listing-error retry and subtree exclusion are `008_traversal`.

## $REQ_IDs

- `020.1` -- When `--max-copies` is not given, at most 10 file copies are active at one time across the whole run.
- `020.2` -- At most `--max-copies` file copies are active at one time across the whole run.
- `020.3` -- A file copy counts against the `--max-copies` limit regardless of peer scheme (`file://` to `file://`, `file://` to `sftp://`, `sftp://` to `file://`, or `sftp://` to `sftp://`).
- `020.4` -- Directory listing, snapshot download/upload, directory creation, and BAK/TMP/SWAP cleanup proceed while `--max-copies` file copies are already active.
- `020.5` -- Copy work for an early scanned directory begins while later directories are still being scanned.
- `020.6` -- Directory listings for all reachable peers at a given directory level are issued concurrently rather than one after another.
- `020.7` -- `--retries-copy` is the maximum total number of tries for a queued copy, counting the first try.
- `020.8` -- When `--retries-copy` is not given, a queued copy is tried at most 3 times in total.
- `020.9` -- A copy try that fails before the copy reaches its try limit moves the copy to the back of the queue, and other queued work continues.
- `020.10` -- A copy whose try count reaches `--retries-copy` is marked failed for the run and is not requeued.
- `020.11` -- One copy's failed tries do not reduce the number of tries available to another copy.
- `020.12` -- The copy try limit applies identically to local copies, SFTP copies, and mixed-scheme copies.
- `020.13` -- A file copy uses buffering whose total size is independent of the size of the file being copied.
- `020.14` -- A file copy begins writing to the destination before the entire source file has been read into memory.
- `020.15` -- A copy between two local filesystem peers replaces the destination through the SWAP staging path rather than writing the destination in place.
