# 03_ignore-rules: `.syncignore` patterns and built-in excludes

## Behavior

KitchenSync excludes files from sync via two mechanisms: per-directory `.syncignore` files (gitignore-style patterns, synced as normal entries) and a hard-coded set of built-in excludes (`.kitchensync/` metadata, symbolic links, special files). `.git/` is excluded by default but can be re-enabled per-directory via `.syncignore`. Derived from `specs/ignore.md` and `specs/multi-tree-sync.md` §"Algorithm" Phase 2b and §"Built-in Excludes".

## $REQ_IDs
- `03.43` — A `.syncignore` file in the union at a directory level is decided and synced first, using the normal decision rules, before other entries are filtered.
- `03.44` — The winning `.syncignore`'s patterns are combined with accumulated parent-directory ignore rules and applied to the remaining union entries; matching entries are skipped (no decisions, no copies, no snapshot updates).
- `03.45` — `.syncignore` itself is never filtered out by parent ignore rules — it is always resolved before filtering applies at its level.
- `03.46` — `.syncignore` accepts gitignore pattern syntax: extension globs (`*.log`), directory patterns (`build/`), negation (`!important.log`), and `**` for any-subdirectory matches.
- `03.47` — Ignore rules accumulate down the tree: patterns at deeper levels add to and can override patterns from parent directories.
- `03.48` — If reading the winning `.syncignore` fails at a directory level, a warning is logged and only the accumulated parent ignore rules are used for that directory.
- `03.49` — `.kitchensync/` directories are always excluded from sync and cannot be overridden by any `.syncignore` pattern.
- `03.50` — `.git/` directories are excluded by default but can be synced by adding a negating entry such as `!.git/` to `.syncignore`.
- `03.51` — Symbolic links (both file and directory targets) are always excluded from sync and cannot be overridden by any `.syncignore` pattern.
- `03.52` — Special files (devices, FIFOs, sockets) are always excluded from sync and cannot be overridden by any `.syncignore` pattern.
