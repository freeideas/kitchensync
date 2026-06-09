# 024_dry-run: Dry-run mode

## Behavior
This concern derives from `specs/sync.md` sections "Dry Run", "Operation Queue"
(dry-run notes), the dry-run clauses of "Startup", and the dry-run notes in
`specs/multi-tree-sync.md` ("SWAP Recovery During Traversal", "BAK/TMP Cleanup
During Traversal", "Subordinate Peers") and `specs/database.md`.

It covers the cross-cutting `--dry-run` behavior: the run is made as realistic as
possible while making no change to any peer. KitchenSync still connects,
downloads snapshots as-is (skipping peer-side snapshot SWAP recovery), lists
directories, reads source files for queued copies, updates the local temp
snapshot databases, exercises the copy queue and try limits, and emits the same
`C`/`X` lines, with the phrase `dry run` appearing at least once on stdout. It
covers what is suppressed: missing peer roots and parents are not created
(treated as unreachable); no TMP, SWAP, or BAK directories are created on peers;
no destination files are written, displaced, or deleted; no modification times
are set on peers; updated local snapshots are not uploaded; and peer-side BAK/TMP
cleanup and SWAP recovery are skipped. Local temp databases may still be written
because they are local working state.

The normal-run counterparts of each suppressed action are defined in their own
categories (for example `016_snapshot-storage`, `019_swap-replacement`,
`021_staging-and-displacement`, `005_connection-establishment`).

## $REQ_IDs

- `024.1` -- `--dry-run` connects to peers as a normal run does.
- `024.2` -- `--dry-run` skips peer-side snapshot SWAP recovery at startup.
- `024.3` -- `--dry-run` downloads each reachable peer's live `.kitchensync/snapshot.db` as-is.
- `024.4` -- `--dry-run` lists peer directories.
- `024.5` -- `--dry-run` reads source files for queued copies.
- `024.6` -- `--dry-run` creates and updates the local temp snapshot databases.
- `024.7` -- `--dry-run` queued copies acquire copy slots subject to the global active-copy limit.
- `024.8` -- `--dry-run` applies the `--retries-copy` try limit to queued copies.
- `024.9` -- `--dry-run` emits the same `C`/`X` progress lines as a normal run.
- `024.10` -- `--dry-run` prints the phrase `dry run` at least once on stdout.
- `024.11` -- `--dry-run` treats a peer whose root path does not already exist as unreachable rather than creating the missing root or parents.
- `024.12` -- `--dry-run` creates no directories on peers.
- `024.13` -- `--dry-run` creates no TMP, SWAP, or BAK directories on peers.
- `024.14` -- `--dry-run` writes no destination files on peers.
- `024.15` -- `--dry-run` displaces no destination files on peers.
- `024.16` -- `--dry-run` deletes no destination files on peers.
- `024.17` -- `--dry-run` sets no modification times on peers.
- `024.18` -- `--dry-run` does not upload updated local temp snapshots back to peers.
- `024.19` -- `--dry-run` skips peer-side BAK/TMP cleanup during traversal.
- `024.20` -- `--dry-run` skips peer-side SWAP recovery during traversal.
