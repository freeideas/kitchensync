# 008_traversal: Combined-tree walk

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Overview" and
"Algorithm" (the structural walk, excluding the per-entry classification and
decision logic), and `specs/sync.md` section "Case Sensitivity".

It covers the shape of the single recursive walk over N peer trees: at each
directory level, list all peers in parallel, union the live entry names
(contributing peers drive the union; subordinate names are included only for
cleanup; the snapshot never contributes entries), process entries in
deterministic case-insensitive lexicographic order with the original
case-sensitive name as tie-breaker, and recurse pre-order so every entry in a
directory is decided and acted on before any subdirectory is entered. It covers
that displacement runs inline during the walk (not in the copy queue), that a
directory chosen for displacement is moved as a single subtree rename and is not
recursed into, and that only peers keeping a directory participate in its
recursion. It also covers listing-error handling for a subtree: retry listing up
to `--retries-list` total tries, then exclude that peer from the directory and
its whole subtree without modifying its snapshot rows; if the canon peer's
listing fails, skip decisions for that subtree for all peers; if all
contributing peers fail listing for a directory, skip that subtree entirely. It
covers that filenames are preserved exactly as reported, so syncing across
case-sensitive and case-insensitive filesystems may collapse or duplicate
case-only variants (recoverable from BAK/).

How each entry's winner is chosen is `010_entry-classification`,
`011_decision-rules`, and `012_directory-and-type-decisions`. Concurrency of the
parallel listings and the copy slot limit are `020_copy-execution`. Per-peer
SWAP recovery before listing is `019_swap-replacement`.

## $REQ_IDs

- `008.1` -- Within a directory, KitchenSync processes entries in case-insensitive lexicographic order, using the original case-sensitive name as the tie-breaker.
- `008.2` -- KitchenSync acts on every entry in a directory before it enters any subdirectory of that directory.
- `008.3` -- An entry that appears in any contributing peer's live listing is visited during the walk for that directory.
- `008.4` -- An entry that appears only in subordinate peers' live listings is visited during the walk so it can be brought into conformance.
- `008.5` -- An entry that appears only in snapshot rows, and in no peer's live listing, is not added to the set of entries processed during the walk.
- `008.6` -- A displacement required before a file copy into the same path (a type conflict) completes during the walk, so the dependent copy succeeds within the same run.
- `008.7` -- A directory chosen for displacement is moved to BAK/ as a single rename that preserves its entire subtree.
- `008.8` -- KitchenSync does not recurse into a directory that is being displaced on a peer; entries inside that directory are not processed individually on that peer.
- `008.9` -- When a directory is kept on some peers and displaced on others, only the peers keeping the directory have its children synchronized.
- `008.10` -- When listing a directory on a reachable peer fails, KitchenSync retries that listing up to `--retries-list` total tries.
- `008.11` -- After a peer's listing fails on all allowed tries, no files or directories are created, deleted, displaced, or copied on that peer under the failed subtree during the run.
- `008.12` -- After a peer's listing fails on all allowed tries, that peer's snapshot rows for the failed subtree are not modified.
- `008.13` -- When the canon peer's listing fails on all allowed tries, no peer's files under that subtree are modified during the run.
- `008.14` -- When the canon peer's listing fails on all allowed tries, no peer's snapshot rows under that subtree are modified during the run.
- `008.15` -- When every contributing peer fails listing for a directory, KitchenSync skips that subtree entirely and does not displace subordinate peers' files under it.
- `008.16` -- Filenames are preserved exactly as the filesystem reports them; KitchenSync does not alter the case or characters of an entry's name when syncing it.

## Notes

Bullets 008.3-008.6 assert traversal-shape properties (which entries are visited
and when displacement runs) using observable sync outcomes as proof of
visitation and timing. The winner-selection that decides those outcomes belongs
to `010`-`012`; keep those decision rules out of this file even though the
observable proof of traversal here is produced by a decision.
