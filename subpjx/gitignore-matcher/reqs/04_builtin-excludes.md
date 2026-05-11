# 04_builtin-excludes: Built-in excludes for .kitchensync, .git, symlinks, and special files

## Behavior
Beyond user-supplied `.syncignore` patterns, the matcher applies built-in excludes. `.kitchensync` is always ignored at any depth and cannot be re-included by a user negation. `.git/` is ignored by default but is overridable by a user `!.git/` pattern. Symbolic links and special files (devices, FIFOs, sockets) cannot appear in a path string alone, so the matcher exposes `is_ignored_entry(m, path, kind)` to carry the `EntryKind` and always ignores symlink and special entries. Derived from `SPEC.md` §"Built-in excludes".

## $REQ_IDs
- `04.1` — A path component named `.kitchensync` at any depth is ignored regardless of `.syncignore` contents.
- `04.2` — Paths located inside a `.kitchensync` directory are ignored regardless of `.syncignore` contents.
- `04.3` — A `!.kitchensync` pattern in a `.syncignore` does not re-include `.kitchensync`; the built-in exclude cannot be negated.
- `04.4` — `.git/` is ignored by default when no user pattern overrides it.
- `04.5` — A `!.git/` pattern in a `.syncignore` re-includes `.git`, overriding the default exclude.
- `04.6` — `is_ignored_entry(m, path, kind)` returns true when `kind` is `symlink`, regardless of path or matcher contents.
- `04.7` — `is_ignored_entry(m, path, kind)` returns true when `kind` is `special`, regardless of path or matcher contents.
- `04.8` — `is_ignored_entry` with `kind` of `file` or `dir` returns the same result as `is_ignored` would for that path with the corresponding `is_dir` flag.

## Notes
The `.git/` default is modeled as a deepest-priority implicit pattern at the sync root so that the normal "last matching pattern wins" rule lets a user `!.git/` override it; the `.kitchensync` built-in sits outside that mechanism and is unconditional.
