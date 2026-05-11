# 03_anchoring-and-directory-only: Leading-slash anchoring and trailing-slash directory-only restrict pattern scope

## Behavior
A leading `/` anchors a pattern to the directory whose `.syncignore` produced it (the `scope_dir` passed to `push_scope`), so the pattern does not float deeper into the tree. A trailing `/` restricts a pattern to match only directory entries, so files of the same name are not affected. Derived from `SPEC.md` §"Compiling pattern text" (leading/trailing slash bullets) and §"Stacking ignore scopes" (anchored patterns interpreted relative to `scope_dir`).

## $REQ_IDs
- `03.7` — A pattern with a leading `/` is anchored to its `scope_dir`: it matches a path located directly at that level but does not match the same name appearing in a nested subdirectory of `scope_dir`.
- `03.8` — A pattern without a leading `/` matches the name at any depth within (and below) its `scope_dir`.
- `03.9` — A pattern with a trailing `/` matches a path only when `is_ignored` is called with `is_dir=true`.
- `03.10` — A directory-only pattern (trailing `/`) does not match a regular file of the same name (`is_ignored` returns false when `is_dir=false`).

## Notes
Anchoring is relative to the scope, not to the sync root: a pattern compiled into a deeper scope's PatternSet anchors to that scope's directory when push_scope is called with the appropriate `scope_dir`.
