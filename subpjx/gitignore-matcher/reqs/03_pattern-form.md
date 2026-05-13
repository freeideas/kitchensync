# 03_pattern-form: anchoring and directory-only restrictions

## Behavior
A pattern's textual form decides where it is allowed to match. Slash placement controls anchoring: patterns with no internal `/` (or only a trailing one) match at any depth below the declaring scope, while patterns with an internal `/` are anchored at the declaring scope (with a leading `/` explicitly anchoring at the scope's root). A trailing `/` further restricts the pattern to directories only, so candidates with `is_directory` false do not match such patterns. Derived from `./specs/SPEC.md` section "Pattern syntax".

## $REQ_IDs
- `03.1` — A pattern containing no `/`, or with only a trailing `/`, matches at any depth below the scope where it was declared.
- `03.2` — A pattern containing `/` somewhere other than at the very end is anchored at the declaring scope (it does not float to deeper directories).
- `03.3` — A pattern with a leading `/` is anchored at the declaring scope's root.
- `03.4` — A pattern with a trailing `/` matches only candidates for which `is_directory` is true; the same candidate name with `is_directory` false does not match.

## Notes
"Declaring scope" is the `scope` of the stack entry the pattern came from (see [[02_match-stack]]). Glob tokens are listed separately in [[03_glob-tokens]].
