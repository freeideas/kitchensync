# 04_builtin-excludes: Layered .kitchensync and .git built-in rules

## Behavior
Two built-in exclusions are layered on top of user-rule evaluation. Any path with a segment equal to `.kitchensync` is ignored unconditionally and cannot be negated. A path whose first segment is `.git` is ignored by default when the user-rule outcome is "not ignored" and no user negation pattern applied — making `.git/` an implicit deepest-priority ignore that an explicit user `!`-pattern can override. Derived from SPEC.md §"Querying" (built-in excludes paragraph).

## $REQ_IDs
- `04.1` — A path with any segment equal to `.kitchensync` is ignored regardless of user rules.
- `04.2` — The `.kitchensync` built-in cannot be overridden by a user negation pattern.
- `04.3` — When no user pattern applies, a path equal to `.git` or starting with `.git/` is ignored.
- `04.4` — A user negation pattern that applies to a `.git`-prefixed path overrides the `.git` built-in, leaving the path not ignored.
