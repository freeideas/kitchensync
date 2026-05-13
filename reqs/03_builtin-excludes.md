# 03_builtin-excludes: Built-in exclusions from sync

## Behavior

Certain entries are always excluded from sync regardless of `.syncignore` contents. `.kitchensync/`, symbolic links, and special files cannot be re-enabled by any `.syncignore` rule. `.git/` is excluded by default. Derived from `multi-tree-sync.md` §"Built-in Excludes", `ignore.md` §"Symlinks" and §"Built-in Excludes", and `sync.md` §"Peer Transports".

## $REQ_IDs

- `03.47` — `.kitchensync/` directories are never synced between peers and cannot be re-enabled by any `.syncignore` rule.
- `03.48` — Symbolic links (files and directories) are never synced between peers and cannot be re-enabled by any `.syncignore` rule.
- `03.49` — Special filesystem entries (devices, FIFOs, sockets) are never synced between peers and cannot be re-enabled by any `.syncignore` rule.
- `03.50` — `.git/` directories are excluded from sync by default.
- `03.51` — A `!.git/` entry in a `.syncignore` file overrides the default exclusion: `.git/` directories at that level (and below, per the usual hierarchy) participate in sync.

## Notes

`stat` on a symbolic link or special file returns "not found", consistent with their omission from listings.
