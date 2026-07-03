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

## Notes
This category owns which entries are visited and in what order. The choice of
winning file or directory state belongs to `007_reconciliation-decisions`.

## $REQ_IDs
