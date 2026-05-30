# 007_traversal-and-excludes: Combined-tree traversal and excludes

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Overview", "Algorithm", "Built-in Excludes", "Offline Peers", and "Excludes", `specs/sync.md` sections "Command-Line Excludes" and "Errors", and `specs/concurrency.md` section "Directory Listing". It covers recursive combined-tree walk ordering, parallel directory listing, union construction, deterministic entry ordering, built-in excludes, command-line exclude effects, listing retry behavior, listing failure subtree exclusion, offline peer exclusion, and the rule that traversal is driven by live listings rather than snapshot-only rows.

## $REQ_IDs
- `007.1` -- KitchenSync synchronizes peer trees with a single recursive combined-tree walk.
- `007.2` -- At each directory level, KitchenSync issues directory listings for every reachable peer that is still participating in that directory level before awaiting any listing result for that level.
- `007.3` -- KitchenSync decides and acts on every entry in a directory before recursing into any subdirectory of that directory.
- `007.4` -- KitchenSync builds each directory's traversal entry set as a union of listed entry names.
- `007.5` -- Snapshot rows do not add names to a directory's traversal entry set.
- `007.6` -- Listed names from active contributing peers are included in the traversal entry set for that directory.
- `007.7` -- When at least one active contributing peer remains for a directory, listed names from active subordinate peers are included in the traversal entry set for that directory.
- `007.8` -- KitchenSync processes entries within a directory in case-insensitive lexicographic order, using the original case-sensitive name as the tie-breaker.
- `007.9` -- KitchenSync excludes `.kitchensync/` directories from traversal.
- `007.10` -- KitchenSync excludes `.git/` directories from traversal.
- `007.11` -- KitchenSync excludes symbolic-link files and symbolic-link directories from traversal.
- `007.12` -- KitchenSync excludes devices, FIFOs, sockets, and other special files from traversal.
- `007.13` -- Each `-x <relative-path>` option excludes the named relative path from scanning.
- `007.14` -- When `-x <relative-path>` names a file, KitchenSync skips only that file.
- `007.15` -- When `-x <relative-path>` names a directory, KitchenSync skips that directory and all descendants.
- `007.16` -- KitchenSync treats excluded entries as nonexistent for the run's sync decisions.
- `007.17` -- KitchenSync leaves existing excluded files and directories untouched on every peer.
- `007.18` -- KitchenSync does not create or copy excluded paths onto peers where those paths are absent.
- `007.19` -- KitchenSync does not update snapshot rows for excluded paths during the run.
- `007.20` -- If listing a directory fails on a reachable peer, KitchenSync retries that same listing up to `--retries-list` total attempts.
- `007.21` -- If listing a directory still fails on a reachable peer after all allowed attempts, KitchenSync logs the failed listing at error level.
- `007.22` -- If listing a directory still fails on a reachable peer after all allowed attempts, KitchenSync excludes that peer from sync decisions for that directory and its entire subtree.
- `007.23` -- If listing a directory still fails on a reachable peer after all allowed attempts, KitchenSync leaves that peer's snapshot rows for that directory subtree unmodified.
- `007.24` -- If listing a directory still fails on a reachable peer after all allowed attempts, KitchenSync does not create, delete, displace, or copy files or directories on that peer under the failed subtree during that run.
- `007.25` -- If listing a directory still fails on the canon peer after all allowed attempts, KitchenSync skips decisions for that directory and its entire subtree for all peers.
- `007.26` -- If listing a directory still fails on the canon peer after all allowed attempts, KitchenSync leaves every peer's files and directories under that subtree unmodified during that run.
- `007.27` -- If listing a directory still fails on the canon peer after all allowed attempts, KitchenSync leaves every peer's snapshot rows for that directory subtree unmodified during that run.
- `007.28` -- If all contributing peers fail listing for a directory, KitchenSync skips decisions for that directory and its entire subtree.
- `007.29` -- If all contributing peers fail listing for a directory, KitchenSync does not process entries under that directory subtree.
- `007.30` -- If all contributing peers fail listing for a directory, KitchenSync does not displace subordinate peer files under that directory subtree.
- `007.31` -- A peer excluded because of a directory-listing failure participates in listings and sync decisions on a later run when listing that subtree succeeds.
- `007.32` -- KitchenSync logs each skipped unreachable peer at error level during the run.
- `007.33` -- KitchenSync excludes unreachable peers from all directory listings during the run.
- `007.34` -- KitchenSync excludes unreachable peers from sync decisions during the run.
- `007.35` -- KitchenSync leaves snapshot rows for unreachable peers unmodified during the run.
- `007.36` -- A peer excluded because it was unreachable participates in listings and sync decisions on a later run when it is reachable.

## Notes
This category owns which entries are visited and visible to decisions. It does not own the decision rules for visited entries or the row update semantics after an action succeeds.
