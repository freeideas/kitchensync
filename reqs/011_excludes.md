# 011_excludes: Built-in and command-line exclusion behavior

## Behavior
This concern derives from `specs/sync.md` section "Command-Line Excludes" and
`specs/multi-tree-sync.md` sections "Built-in Excludes" and "Algorithm". It
covers the observable effect of excluded paths and excluded entry types during a
run: excluded entries are not scanned, recursed into, copied, deleted,
displaced, or used for snapshot lookup or update, and built-in excludes cannot
be overridden by command-line input.

## $REQ_IDs
- `011.1` -- Every accepted `-x <relative-path>` value excludes its matching path for that run.
- `011.2` -- When an accepted command-line exclude matches a file, only that file path is excluded by that exclude.
- `011.3` -- When an accepted command-line exclude matches a directory, that directory and all descendants are excluded by that exclude.
- `011.4` -- `.kitchensync/` directories are excluded in every run.
- `011.5` -- `.git/` directories are excluded in every run.
- `011.6` -- Symbolic link files are excluded in every run.
- `011.7` -- Symbolic link directories are excluded in every run.
- `011.8` -- Special files are excluded in every run.
- `011.9` -- Built-in excluded entries remain excluded regardless of command-line excludes supplied for the run.
- `011.10` -- Excluded paths are omitted from sync decisions for the run.
- `011.11` -- Excluded paths are not scanned during the run.
- `011.12` -- Excluded directories are not recursed into during the run.
- `011.13` -- Excluded entries are not copied during the run.
- `011.14` -- Existing excluded entries are not deleted during the run.
- `011.15` -- Existing excluded entries are not displaced during the run.
- `011.16` -- Snapshot rows for excluded paths are not consulted during the run.
- `011.17` -- Snapshot rows for excluded paths are not updated during the run.

## Notes
Argument syntax and validation for `-x` belongs to the command-line categories.
This file covers only the traversal and sync effect of an accepted exclude.
