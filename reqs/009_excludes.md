# 009_excludes: Built-in and command-line excludes

## Behavior
This concern derives from `specs/sync.md` section "Command-Line Excludes" and
`specs/multi-tree-sync.md` sections "Built-in Excludes" and "Excludes" (the
exclude-application step of the algorithm).

It covers which entries are removed from the entry union before decisions are
made and the observable consequence of exclusion. Built-in excludes are always
applied: `.kitchensync/` directories, `.git/` directories, symbolic links, and
special files. Command-line `-x <relative-path>` excludes are applied in
addition and cannot include or override the built-in excludes. An excluded file
is skipped; an excluded directory and its entire subtree are skipped. Excluded
paths are treated as if they do not exist for the run: they are not scanned,
copied, deleted, or displaced; their snapshot rows are neither consulted nor
updated; and existing excluded entries on any peer are left untouched.

The slash-path syntax accepted for `-x` and its validation are `001_command-line`.
The transport-level omission of symlinks and special files from `list_dir`/`stat`
results is `022_transports`.

## $REQ_IDs

- `009.1` -- A `.kitchensync/` directory present on one peer is not copied to peers that lack it.
- `009.2` -- A `.git/` directory present on one peer is not copied to peers that lack it.
- `009.3` -- A symbolic link present on one peer is not copied to other peers.
- `009.4` -- A special file (device, FIFO, or socket) present on one peer is not copied to other peers.
- `009.5` -- A path supplied with `-x` that exists on one peer is not copied to peers that lack it.
- `009.6` -- Command-line `-x` excludes take effect in addition to the built-in excludes within the same run.
- `009.7` -- An excluded entry that already exists on a peer is left in place, neither deleted nor displaced to BAK/.
- `009.8` -- An excluded directory and all of its descendants are skipped, so no descendant is copied, deleted, or displaced on any peer.
- `009.9` -- No snapshot row is created or updated for an excluded path during the run.

## Notes

- Symbolic links and special files appear here as built-in excludes because the
  category plan lists them and their end-to-end "never synced" effect is
  observable through the CLI. The transport-level mechanism that omits them from
  `list_dir`/`stat` is owned by `022_transports`; this file asserts only the
  observable consequence, not the omission step.
