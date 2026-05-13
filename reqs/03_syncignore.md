# 03_syncignore: .syncignore exclusion rules

## Behavior

Any directory may contain a `.syncignore` file with gitignore-style patterns; matching entries in that directory and its subdirectories are excluded from sync. `.syncignore` files themselves are synced under the normal decision rules and are never subject to ignore-pattern filtering at their own level. Patterns from deeper levels add to and may override patterns from parent levels. Derived from `ignore.md` (§"Configuration", §"Resolution During the Multi-Tree Walk", §"Pattern Format", §"Hierarchy") and `multi-tree-sync.md` §"Algorithm" (Phase 2b).

## $REQ_IDs

- `03.40` — A `.syncignore` file at a directory level is itself synced across peers using the normal decision rules.
- `03.41` — A `*.ext` pattern in `.syncignore` excludes entries with that file extension from sync.
- `03.42` — A `name/` pattern in `.syncignore` excludes a directory entry with that name from sync.
- `03.43` — A `**/name` pattern in `.syncignore` excludes entries with that name in any subdirectory from sync.
- `03.44` — Patterns from a `.syncignore` in a child directory are combined with the accumulated rules from parent-directory `.syncignore` files when filtering entries within that child directory.
- `03.45` — An entry matching an accumulated ignore pattern is not copied or displaced on any peer.
- `03.94` — An entry matching an accumulated ignore pattern produces no snapshot-row create or update on any peer.
- `03.46` — A `.syncignore` file is never excluded by an accumulated ignore pattern; it is always considered for sync at its directory level.
- `03.88` — A `!pattern` line in a child-directory `.syncignore` un-ignores entries that a parent-directory `.syncignore` rule would otherwise ignore.
- `03.89` — If reading the winning `.syncignore` at a directory level fails, a warning is logged.
- `03.95` — If reading the winning `.syncignore` at a directory level fails, entries in that directory are filtered using only the accumulated parent-level ignore rules.

## Notes

Built-in exclusions that cannot be overridden by `.syncignore` (`.kitchensync/`, symlinks, special files) and the `!.git/` override of the default `.git/` exclusion are covered in `03_builtin-excludes.md`.
