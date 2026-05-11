# 02_matcher-basics: Matchers are built by stacking scopes and queried with is_ignored

## Behavior
Callers build a Matcher by starting from `empty_matcher()` and stacking PatternSets onto it via `push_scope(parent, scope_dir, set)`, one per directory level of the multi-tree walk. They then query the Matcher with `is_ignored(m, path, is_dir)` to decide whether a relative path is ignored. push_scope is purely functional — it returns a new Matcher and never mutates its parent — and precedence follows gitignore's "last matching pattern wins" rule across the entire scope stack. Derived from `SPEC.md` §"Stacking ignore scopes" and §"Querying the matcher".

## $REQ_IDs
- `02.1` — `empty_matcher()` returns a Matcher against which `is_ignored` returns false for arbitrary paths that no built-in exclude covers.
- `02.2` — `push_scope(parent, scope_dir, set)` returns a new Matcher whose rules include the patterns of `set` interpreted at `scope_dir`.
- `02.3` — `push_scope` does not mutate the parent Matcher; querying the parent after pushing yields the same results as querying it before.
- `02.4` — `is_ignored(m, path, is_dir)` returns true for a path that matches a literal (non-wildcard, non-negated) pattern present in `m`.
- `02.5` — A pattern beginning with `!` is a negation; when it matches a path that an earlier pattern excluded, the path is no longer ignored.
- `02.6` — When multiple patterns match a path, the last matching pattern across the entire scope stack determines whether the path is ignored.

## Notes
Stacking order matters across scopes as well as within a scope: deeper-scope patterns are evaluated after shallower-scope patterns, so a deeper `!pattern` can re-include something a shallower scope excluded — observable via `02.6`.
