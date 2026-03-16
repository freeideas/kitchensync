# Ignore Rules

How KitchenSync excludes files from synchronization.

## Configuration

Any directory may contain a `.syncignore` file listing patterns of files and directories to exclude from sync. Patterns apply to the directory containing the `.syncignore` and its subdirectories.

`.syncignore` files are themselves synced, so ignore rules stay consistent across all peers.

All directory walks — local and peer — apply the **local** `.syncignore` rules. Peer `.syncignore` files are not read during sync; they only take effect when that peer runs its own KitchenSync instance.

## Pattern Format

Uses the same pattern syntax as `.gitignore`:

- `*.log` — match by extension
- `build/` — ignore a directory
- `!important.log` — negate a previous pattern
- `**/temp` — match in any subdirectory

## Hierarchy

Ignore files at deeper levels add to (and can override) patterns from parent directories, just like `.gitignore`.

## Symlinks

Symbolic links are always skipped — both files and directories. During local and peer walks, symlinks are not followed, not included in the file list, and not synced. This cannot be overridden.

Why: following symlinks could sync files outside the sync root or create infinite loops. Symlink targets may not exist on other peers. On Windows, creating symlinks requires elevated privileges.

## Built-in Excludes

The following are always excluded regardless of ignore files:

- `.kitchensync/` directories
- `.git/` directories
- Symbolic links (files and directories)

A `.syncignore` file may negate the `.git/` exclusion (e.g. `!.git/`) to force syncing it. The `.kitchensync/` and symlink exclusions cannot be overridden.
