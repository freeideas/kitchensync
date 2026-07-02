# 010_tree-walk-and-listing: Combined-tree traversal and listing failures

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Overview" and
"Algorithm", `specs/concurrency.md` section "Directory Listing", and
`specs/sync.md` sections "Run" and "Errors". It covers the recursive
combined-tree walk, pre-order processing, deterministic entry ordering,
concurrent directory listing at each level, listing retry limits, subtree
exclusion after listing failure, conservative handling when canon or survival
evidence cannot be listed, and the behavior of the current directory subtree
after per-directory listing errors.

## $REQ_IDs
- `010.1` -- A sync run traverses peer contents as one recursive combined tree.
- `010.2` -- At each traversed directory path, KitchenSync starts directory listing operations for every reachable peer in that subtree before awaiting any listing result.
- `010.3` -- At each traversed directory path, KitchenSync forms the entry names to process from live peer listings, not from snapshot-only paths.
- `010.4` -- At each traversed directory path, KitchenSync includes live entry names from every active contributing peer in the entry set for that path.
- `010.5` -- At each traversed directory path, KitchenSync includes live entry names from active subordinate peers in the entry set for that path.
- `010.6` -- Within one directory, KitchenSync processes entry names in case-insensitive lexicographic order, using the original case-sensitive name as the tie-breaker.
- `010.7` -- KitchenSync finishes processing every entry in a directory before recursing into any child directory.
- `010.8` -- KitchenSync does not recurse into a directory on a peer after displacing that directory on that peer.
- `010.9` -- KitchenSync recurses into a child directory only with peers that keep or create that child directory.
- `010.10` -- When a directory listing fails on a reachable peer, KitchenSync tries that same listing up to `--retries-list` total times.
- `010.11` -- When a peer's directory listing still fails after all allowed tries, KitchenSync logs the failed peer and path at error level.
- `010.12` -- When a non-canon peer's directory listing still fails after all allowed tries and at least one contributing peer remains active, KitchenSync continues processing that directory with the remaining active peers.
- `010.13` -- When a peer's directory listing still fails after all allowed tries, KitchenSync excludes that peer from decisions for that directory and every descendant path during that run.
- `010.14` -- When a peer's directory listing still fails after all allowed tries, KitchenSync does not modify files or directories under the failed subtree on that peer during that run.
- `010.15` -- When a peer's directory listing still fails after all allowed tries, KitchenSync does not modify that peer's snapshot rows for the failed subtree during that run.
- `010.16` -- When the canon peer's directory listing still fails after all allowed tries, KitchenSync skips decisions for that directory and every descendant path for all peers.
- `010.17` -- When the canon peer's directory listing still fails after all allowed tries, KitchenSync does not modify files, directories, or snapshot rows under that subtree on any peer during that run.
- `010.18` -- When every contributing peer still fails listing a directory after all allowed tries, KitchenSync skips decisions for that directory and every descendant path.
- `010.19` -- When every contributing peer still fails listing a directory after all allowed tries, KitchenSync does not displace subordinate peer files under that subtree during that run.
- `010.20` -- A peer excluded because of a directory listing failure in one run participates normally on a later run when listing that path succeeds.
- `010.21` -- When survival-evidence listing for a directory fails on a peer after all allowed tries, KitchenSync skips decisions for that directory and every descendant path for all peers during that run.

## Notes
This file covers traversal control and visibility after listing succeeds or
fails. Entry decisions, excludes, and filesystem actions are separate
categories.
