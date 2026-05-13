# 03_negation: re-including paths with `!` and the parent-directory restriction

## Behavior
A pattern with a leading `!` is a negation: when it matches a path that some earlier-applied pattern had classified as `Ignored`, the path is reclassified as `NotIgnored`. Negation has one important limit: a negation cannot re-include a path whose strict ancestor directory is itself classified `Ignored`. Determining that ancestor classification is itself a recursive `match`-style check, but restricted to patterns whose scope sits at or above the ancestor being tested, and the ancestor is treated as a directory for that check. Derived from `./specs/SPEC.md` section "API surface › Match" (negation/parent-directory paragraphs) and "Pattern syntax" (leading `!`).

## $REQ_IDs
- `03.1` — A negated pattern (leading `!`) that matches the candidate reclassifies a path that would otherwise be `Ignored` as `NotIgnored`.
- `03.2` — If any strict ancestor directory of the candidate would itself be classified `Ignored`, the candidate is `Ignored` regardless of any negation that would otherwise apply.
- `03.3` — When classifying an ancestor directory for the parent-directory restriction, the ancestor is treated as a directory (`is_directory` true), so directory-only patterns can apply to it.
- `03.4` — When classifying an ancestor directory for the parent-directory restriction, only patterns whose scope is an ancestor of (or equal to) the ancestor being tested are considered.

## Notes
"Strict ancestor" excludes the candidate itself. The parent-directory restriction is the reason a pattern like `foo/` followed by `!foo/bar` does not re-include `foo/bar`. Order-of-application semantics for the non-ancestor case are covered in [[02_match-stack]]; pattern form (what makes a pattern directory-only or anchored) is in [[03_pattern-form]].
