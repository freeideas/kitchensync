# 006_tree-traversal-and-excludes: Tree traversal and excludes

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Overview",
"Algorithm", "Built-in Excludes", "Excludes", "BAK/TMP Cleanup During
Traversal", "Orphaned Snapshot Rows", and "Offline Peers", `specs/sync.md`
sections "Command-Line Excludes", "Run", "Dry Run", and "Operation Queue", and
`specs/SCENARIOS.md` property "P-03: Peer Metadata Is Never Synced". It covers
the observable recursive combined-tree walk, pre-order entry handling,
deterministic entry ordering, concurrent per-directory listings, listing retry
and subtree exclusion behavior, built-in and command-line excludes, omission of
excluded paths from snapshot work, and the handling of offline peers.

## $REQ_IDs
- `006.1` -- A sync run walks the accepted peer file trees as one recursive combined-tree traversal.
- `006.2` -- At each visited directory level, KitchenSync lists that directory on every active peer for that subtree concurrently.
- `006.3` -- The traversal entry set for a directory is the union of names from live listings of active contributing peers and active subordinate peers.
- `006.4` -- Snapshot rows do not add entries to the traversal entry set.
- `006.5` -- Built-in excludes and command-line excludes are removed from the traversal entry set before decisions are made.
- `006.6` -- Entries within one directory are processed in case-insensitive lexicographic order, using the original case-sensitive name as the tie-breaker.
- `006.7` -- KitchenSync processes every entry in a directory before recursing into any child directory from that directory.
- `006.8` -- KitchenSync does not recurse into a directory that is displaced during traversal.
- `006.9` -- Only peers that keep a directory participate in recursion into that directory.
- `006.10` -- A failed directory listing is tried no more than `--retries-list` total times for that directory on that peer.
- `006.11` -- After all allowed listing tries fail for a non-canon peer, that peer is excluded from decisions for that directory and its subtree.
- `006.12` -- After all allowed listing tries fail for a peer, KitchenSync creates no files or directories under the failed subtree on that peer during that run.
- `006.13` -- After all allowed listing tries fail for a peer, KitchenSync deletes no files or directories under the failed subtree on that peer during that run.
- `006.14` -- After all allowed listing tries fail for a peer, KitchenSync displaces no files or directories under the failed subtree on that peer during that run.
- `006.15` -- After all allowed listing tries fail for a peer, KitchenSync copies no files under the failed subtree to or from that peer during that run.
- `006.16` -- After all allowed listing tries fail for a peer, KitchenSync does not modify that peer's snapshot rows for the failed subtree during that run.
- `006.17` -- If the canon peer's listing fails after all allowed tries for a directory, KitchenSync skips decisions for that directory and its subtree on all peers during that run.
- `006.18` -- If the canon peer's listing fails after all allowed tries for a directory, KitchenSync modifies no peer files or directories under that subtree during that run.
- `006.19` -- If the canon peer's listing fails after all allowed tries for a directory, KitchenSync modifies no peer snapshot rows under that subtree during that run.
- `006.20` -- If every contributing peer's listing fails after all allowed tries for a directory, KitchenSync processes no entries in that directory or its subtree during that run.
- `006.21` -- An unreachable peer is excluded from traversal listings and decisions for the run.
- `006.22` -- KitchenSync does not modify snapshot rows for an unreachable peer during the run.
- `006.23` -- A peer that was unreachable in one run participates in traversal on a later run when it is reachable.
- `006.24` -- KitchenSync treats `.kitchensync/` directories as built-in excludes from the user file tree.
- `006.25` -- KitchenSync treats `.git/` directories as built-in excludes from the user file tree.
- `006.26` -- KitchenSync treats symbolic links as built-in excludes from the user file tree.
- `006.27` -- KitchenSync treats special files as built-in excludes from the user file tree.
- `006.28` -- Each accepted `-x RELPATH` option excludes that relative path in addition to the built-in excludes.
- `006.29` -- A command-line exclude never makes a built-in excluded path syncable.
- `006.30` -- A command-line exclude that matches a file skips only that file.
- `006.31` -- A command-line exclude that matches a directory skips that directory and all descendants.
- `006.32` -- KitchenSync leaves existing excluded files and directories unchanged on every peer.
- `006.33` -- KitchenSync does not consult existing snapshot rows for excluded paths during the run.
- `006.34` -- KitchenSync does not update snapshot rows for excluded paths during the run.

## Notes
This category owns which entries are visited and in what order. The choice of
winning file or directory state belongs to `007_reconciliation-decisions`.
Orphaned snapshot row deletion belongs to `010_snapshot-row-updates`; this file
only covers that snapshot rows do not add entries to traversal. Retention
cleanup of BAK and TMP contents belongs to `009_recoverable-staging`; this file
only covers excludes from the user file tree.
