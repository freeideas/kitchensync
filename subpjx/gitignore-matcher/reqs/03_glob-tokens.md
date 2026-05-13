# 03_glob-tokens: glob metacharacters and double-star semantics

## Behavior
Within a pattern, several metacharacters give it flexible matching against path strings. `*`, `?`, and character classes match within a single path component (they never cross `/`). The `**` token, used in the specific positions defined by gitignore, lets a pattern span zero or more whole path components. Derived from `./specs/SPEC.md` section "Pattern syntax".

## $REQ_IDs
- `03.1` — `*` matches any run (including empty) of non-`/` characters within a single path component.
- `03.2` — `?` matches exactly one non-`/` character.
- `03.3` — `[abc]` matches exactly one character from the listed set.
- `03.4` — `[a-z]` matches exactly one character in the inclusive range.
- `03.5` — `[!abc]` matches exactly one character that is not in the listed set.
- `03.6` — A leading `**/` lets the rest of the pattern match at any depth below the declaring scope.
- `03.7` — A trailing `/**` matches every path inside the directory matched by the rest of the pattern.
- `03.8` — `a/**/b` matches `b` zero or more directories below `a`.

## Notes
Token semantics interact with anchoring (see [[03_pattern-form]]) — for example, a pattern containing `**` already contains a `/`, so by `3.2` it is anchored unless `**/` is used at the start.
