# Gitignore Pattern Hierarchy

## Purpose
Compile gitignore pattern sets into hierarchy order and return the pattern lines that apply to a relative path.

## Public API
Data shapes:

- `PatternSet`: ordered pattern lines with a `base_path`
- `PatternHierarchy`: compiled pattern set hierarchy
- `HierarchyInput`: relative `path`
- `ScopedPatternLine`: pattern line with its source `base_path` and the input `path` relative to that `base_path`
- `HierarchyResult`: ordered `ScopedPatternLine[]`

Operations:

- `compile_hierarchy(pattern_sets) -> PatternHierarchy`
- `patterns_for_path(hierarchy, input) -> HierarchyResult`

## Behavior
Pattern sets are evaluated in hierarchy order: ancestor `base_path` pattern sets before descendant `base_path` pattern sets.

A pattern set applies only when the input `path` is the directory represented by its `base_path` or a descendant of that directory.

For each applicable pattern set, pattern lines are emitted in their original order with the input path expressed relative to that pattern set's `base_path`.

Pattern text is carried unchanged. Pattern syntax is not interpreted, and ignored or included decisions are not produced.

Hierarchy evaluation performs no filesystem I/O and does not inspect symlinks or special file types.

## Errors
Malformed `base_path` or input `path` returns `invalid_path`.

Pattern syntax is not validated by this API.

Hierarchy evaluation does not return I/O errors.

## Anchoring
`PatternSet`, `base_path`, `PatternHierarchy`, hierarchy ordering, and parent and child rule accumulation are anchored in `ignore.md` "Configuration" and "Hierarchy".

Pattern lines are anchored in `ignore.md` "Pattern Format" and the gitignore pattern syntax documented by Git.

`HierarchyInput.path`, malformed paths, and the no-I/O path-string boundary are anchored in `decomposition.md` "gitignore-matcher".

`ScopedPatternLine` and `HierarchyResult` are anchored in the rule that a pattern applies only to the directory represented by its `base_path` and that directory's descendants.
