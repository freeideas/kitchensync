# 03_pattern-wildcards: Wildcard metacharacters in patterns match per gitignore semantics

## Behavior
Patterns are not purely literal: gitignore-style metacharacters let a single pattern match a family of paths. The single-segment wildcards `*`, `?`, and `[…]` operate within one path component, while `**` is a directory-spanning wildcard recognised in three documented positions. Derived from `SPEC.md` §"Compiling pattern text" (pattern grammar bullets covering `*`, `?`, `[abc]`, and `**`).

## $REQ_IDs
- `03.1` — `*` in a pattern matches any run of characters except `/` within a single path component.
- `03.2` — `?` in a pattern matches exactly one character other than `/`.
- `03.3` — `[abc]` matches a single character drawn from the listed character class.
- `03.4` — A leading `**/` in a pattern allows the remainder to match at any depth within the scope.
- `03.5` — A trailing `/**` in a pattern matches any path located inside the named directory.
- `03.6` — A `/**/` segment within a pattern matches zero or more intermediate directory components.

## Notes
Wildcard behavior is observed through `is_ignored` after the pattern has been compiled and pushed into a Matcher.
