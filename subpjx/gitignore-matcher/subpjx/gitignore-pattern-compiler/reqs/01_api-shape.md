# 01_api-shape: Public compilation API and PatternSet container

## Behavior
The component exposes a small set of pure functions and value types for compiling gitignore-format text into a structured, queryable `PatternSet`. Callers compile a string with `compile_patterns(text)` and receive a `(PatternSet, Diagnostics)` pair; they can construct an empty set with `empty_pattern_set()`, ask for its size with `pattern_count(set)`, and read individual `CompiledPattern` values in source order via `pattern_at(set, index)`. Each `CompiledPattern` also preserves the original source line for round-trip display. Derived from `specs/SPEC.md` sections "API surface", "PatternSet shape", and "Operations on PatternSet".

## $REQ_IDs
- `01.1` — `compile_patterns(text)` returns a `(PatternSet, Diagnostics)` pair.
- `01.2` — `empty_pattern_set()` returns a `PatternSet` whose `pattern_count` is zero.
- `01.3` — `pattern_count(set)` returns the number of compiled patterns in the set.
- `01.4` — `pattern_at(set, i)` returns the i-th `CompiledPattern` using zero-based indexing in source order.
- `01.5` — `CompiledPattern.source` equals the original source line (post-trailing-whitespace-strip, pre-flag-consumption).

## Notes
- Mutation operations on `PatternSet` are not exposed; the spec describes the type as immutable, but immutability is an absence-of-feature claim, so no bullet asserts it.
- Existence of the `is_negation`, `is_anchored`, `is_dir_only`, and `matches` members on `CompiledPattern` is covered by the behavioral tests in `02_*.md` and `03_*.md` that exercise them.
