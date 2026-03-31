# Ignore Rules

How KitchenSync excludes files from synchronization.

## Configuration

Any directory may contain a `.syncignore` file listing patterns of files and directories to exclude from sync. Patterns apply to the directory containing the `.syncignore` and its subdirectories.

`.syncignore` files are themselves synced like any other file — they participate in normal decision rules (canon wins, or newest mod_time wins). This keeps ignore rules consistent across all peers.

If `.syncignore` exists but is a directory (not a file), it is ignored for pattern purposes — no patterns are loaded from it, and it syncs as a normal directory.

## Resolution During the Walk

At each directory level, after listing all peers and computing the union of entry names, `.syncignore` is resolved **before** other entries:

1. If `.syncignore` appears in the union, apply normal decision rules to it first (decide the winning version, enqueue copies to peers that need it).
2. Read the winning `.syncignore` and combine its patterns with the accumulated ignore rules from parent directories.
3. Filter the remaining union entries through the accumulated rules — entries that match are skipped (no decisions, no copies, no snapshot updates).

If no peer has a `.syncignore` at the current level, only parent-level rules (if any) apply.

## Pattern Format

Uses the same pattern syntax as `.gitignore`:

- `*.log` — match by extension
- `build/` — ignore a directory (trailing slash means directory only)
- `!important.log` — negate a previous pattern
- `**/temp` — match in any subdirectory

Go library recommendation: `github.com/sabhiram/go-gitignore` or the gitignore implementation in `github.com/go-git/go-git`.

## Hierarchy

Ignore files at deeper levels add to (and can override) patterns from parent directories, just like `.gitignore`.

## Built-in Excludes

Always excluded regardless of ignore files (cannot be overridden):

- `.kitchensync/` directories — sync metadata must not sync
- Symbolic links (files and directories) — following symlinks could escape the sync root or create loops
- Special files (devices, FIFOs, sockets) — can block reads indefinitely

`.git/` directories are excluded by default. A `.syncignore` file may negate this exclusion (`!.git/`) to force syncing it.

## Symlinks

Symbolic links are always skipped — both files and directories. During local and peer walks, symlinks are not followed, not included in the file list, and not synced. This cannot be overridden.

Why: following symlinks could sync files outside the sync root or create infinite loops. Symlink targets may not exist on other peers. On Windows, creating symlinks requires elevated privileges.
