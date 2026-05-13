# 02_match-stack: deciding ignored/not-ignored across a layered pattern stack

## Behavior
`match` takes a stack of `(scope, Patterns)` pairs (shallowest first), a relative path, and an `is_directory` flag, and returns `Ignored` or `NotIgnored`. It walks the stack in order, applies each entry's patterns at that entry's scope, and the verdict is determined by the most-recently-applied matching pattern (positive → `Ignored`; negation or no match → `NotIgnored`). A scope `D` selects which candidates an entry's patterns even consider: only paths strictly inside `D`, and the components of `D` are stripped from the candidate's path before that entry's patterns are matched. Derived from `./specs/SPEC.md` sections "API surface › Match" and "Empty input".

## $REQ_IDs
- `02.1` — `match` invoked with an empty stack returns `NotIgnored` for every candidate path.
- `02.2` — When no pattern in the stack matches the candidate, the result is `NotIgnored`.
- `02.3` — When the most-recently-applied matching pattern is positive, the result is `Ignored`.
- `02.4` — When the most-recently-applied matching pattern is a negation, the result is `NotIgnored`.
- `02.5` — Stack entries are processed shallowest first; matches contributed by later (deeper) stack entries override matches contributed by earlier (shallower) ones for the same path.
- `02.6` — A pattern declared at scope `D` applies only to candidates strictly inside `D` (candidates whose leading path components are the components of `D`).
- `02.7` — Before a pattern at scope `D` is matched against a candidate, the components of `D` are stripped from the candidate's path.
- `02.8` — A scope of the empty string denotes the caller-chosen root, so its patterns are considered for every candidate.

## Notes
"Most-recently-applied matching pattern" is a single concept covering both intra-`Patterns` order (handled at compile, [[01_compile]]) and cross-entry order (handled here). The negation parent-directory restriction is a separate concern in [[03_negation]].
