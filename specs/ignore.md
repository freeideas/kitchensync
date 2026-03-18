# Ignore Rules

## Configuration

Any directory may contain a `.syncignore` file listing patterns to exclude from sync. Patterns apply to the containing directory and its subdirectories.

During traversal, `.syncignore` files participate in normal sync (they are included in listings and subject to decision rules). Once the authoritative version of a `.syncignore` is decided for a directory, its rules are applied to filter the remaining entries in that directory and its subdirectories. Peer-side `.syncignore` files that lost the decision are still synced (overwritten) but their rules are not used.

## Pattern Format

Same syntax as `.gitignore`:

- `*.log` — match by extension
- `build/` — ignore a directory
- `!important.log` — negate a previous pattern
- `**/temp` — match in any subdirectory

Deeper `.syncignore` files add to and can override parent patterns.

## Built-in Excludes

Always excluded, cannot be overridden:

- `.kitchensync/` directories — sync metadata must not sync
- Symbolic links (files and directories) — following symlinks could escape the sync root or create loops
- Special files (devices, FIFOs, sockets)

Excluded by default, can be overridden with `!` in `.syncignore`:

- `.git/` directories — override with `!.git/`
