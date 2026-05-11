# 02_pattern-matching: Decide whether a pattern applies to a path within its scope

## Behavior
For a candidate path, each `CompiledPattern` in a layer is evaluated against the path's portion below the layer's `scope_dir`, honouring the pattern's `is_anchored` and `is_dir_only` flags. Anchored patterns are scope-rooted; unanchored patterns may match anywhere within the suffix, with body-shape (presence of an internal `/`) deciding whether the body matches the full sub-path or any single segment. Derived from SPEC.md §"Querying" (evaluation rule, step 1).

## $REQ_IDs
- `02.1` — When no pattern in any layer applies to `path` and no built-in exclude applies, `is_ignored` returns false.
- `02.2` — A pattern with `is_dir_only` set does not apply when `is_dir` is false.
- `02.3` — A pattern with `is_dir_only` set applies when `is_dir` is true (subject to its other flags).
- `02.4` — An anchored pattern's body is matched against the portion of `path` that lies below the layer's `scope_dir`.
- `02.5` — An anchored pattern does not apply when `path` is not within the layer's `scope_dir`.
- `02.6` — An unanchored pattern whose body contains no internal `/` applies when the body matches any single segment of the path below `scope_dir`.
- `02.7` — An unanchored pattern whose body contains an internal `/` applies when the body matches the full path below `scope_dir` as a path-shaped match.

## Notes
Pattern compilation (`PatternSet` / `CompiledPattern` semantics including `is_anchored` and `is_dir_only` flags) is an opaque input from a gitignore-conformant compiler (SPEC.md §"Anchoring"); this component only consumes them.
