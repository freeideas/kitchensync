# 03_ignore-rules: `.syncignore` patterns and built-in excludes

## Behavior

Each directory may contain a `.syncignore` listing patterns to exclude from sync, with the same syntax as `.gitignore`. `.syncignore` itself participates in normal decision rules and is resolved before other entries at each level. Some entries are built-in excluded regardless of `.syncignore`. Derived from `./specs/ignore.md` and `./specs/multi-tree-sync.md` (`Built-in Excludes`, `.syncignore` resolution paragraph).

## $REQ_IDs
- `03.61` — A pattern in a directory's `.syncignore` (e.g., `*.log`) excludes matching entries in that directory from being synced (no copy, no displacement, no snapshot row).
- `03.62` — A directory pattern (e.g., `build/`) in `.syncignore` excludes that directory and all its descendants from sync at and below the directory containing the `.syncignore`.
- `03.63` — `.syncignore` patterns at deeper levels add to (and may override) parent-level patterns; `**/temp` matches in any subdirectory.
- `03.64` — A `!pattern` line negates a previous match (the entry is included again).
- `03.65` — A `.syncignore` file itself is synced like any other file (newest wins, canon wins).
- `03.66` — At each directory level, `.syncignore` is decided and, when changed, propagated before other entries are evaluated, so the just-synced rules apply to the rest of that level's entries.
- `03.67` — `.kitchensync/` directories are never listed for sync, never copied between peers, and cannot be re-included by a `.syncignore` rule.
- `03.68` — Symbolic links (file or directory) are never synced and cannot be re-included by any `.syncignore` rule.
- `03.69` — Special files (devices, FIFOs, sockets) are never synced and cannot be re-included by any `.syncignore` rule.
- `03.70` — `.git/` directories are excluded by default but a `.syncignore` line `!.git/` re-includes them.
- `03.71` — If the winning `.syncignore` at a directory level cannot be read, a warning is logged and that directory is processed using only the accumulated parent-level ignore rules (the rest of the run continues normally).
