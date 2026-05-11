# Stack gitignore PatternSets across nested scopes and decide whether a path is ignored

## Purpose
Maintain the hierarchical ignore state in effect at any directory of a directory tree by stacking, in source order, the `PatternSet` produced by each ancestor directory's gitignore file and the current directory's gitignore file. Answer, for any candidate relative path, whether it is ignored, applying gitignore's "last matching pattern wins" precedence across the entire stack, honouring each pattern's anchoring and directory-only flags relative to the scope where it was added, and applying a small set of built-in always-ignored / default-ignored rules on top. This component is pure and does no filesystem I/O. It implements the hierarchical-evaluation portion of gitignore semantics together with this project's built-in exclude rules (SPEC.md §"Built-in Excludes" and the hierarchy and querying sections).

## API surface

### Matcher value

A `Matcher` represents the gitignore rules in effect at a particular directory. It is immutable; building a deeper matcher returns a new value without mutating the parent. Conceptually a `Matcher` is an ordered stack of layers; each layer carries a `scope_dir` (a relative path identifying the directory whose gitignore file produced the layer) and a `PatternSet` (the compiled patterns from that file). Layer order matches the order in which scopes were pushed: outermost / shallowest first, innermost / deepest last.

### Constructing matchers

`empty_matcher() -> Matcher` — the matcher at the sync root before any `.syncignore` has contributed rules. It contains no user layers; only the built-in exclude rules apply.

`push_scope(parent: Matcher, scope_dir: relative-path, set: PatternSet) -> Matcher` — returns a new `Matcher` equal to `parent` with one additional layer appended for `scope_dir` carrying `set`. `scope_dir` is a forward-slash-separated relative path with no leading slash, interpreted relative to the sync root, identifying the directory whose `.syncignore` produced `set` (the empty string denotes the sync root itself). `parent` is not modified. Pushing an empty `PatternSet` is allowed and is equivalent to pushing nothing for matching purposes, but the layer may still be visible to consumers that introspect the stack.

### Querying

`is_ignored(m: Matcher, path: relative-path, is_dir: bool) -> bool` — decides whether `path`, interpreted as a forward-slash-separated relative path with no leading slash anchored at the sync root, is ignored under `m`. `is_dir` indicates whether the candidate is a directory, so that patterns whose `is_dir_only` flag is set match only when `is_dir == true`. Evaluation rule:

1. For each layer in stack order (shallowest to deepest), for each `CompiledPattern` in the layer's `PatternSet` (in source order), determine whether the pattern applies to `path`:
   - If the pattern's `is_dir_only` flag is set and `is_dir` is false, the pattern does not apply.
   - If the pattern's `is_anchored` flag is set, the pattern's body is matched against the suffix of `path` that lies inside the layer's `scope_dir` — that is, `path` must lie within `scope_dir`, and the portion below `scope_dir` is what the pattern body sees. If `path` is not within `scope_dir`, the pattern does not apply.
   - If the pattern's `is_anchored` flag is not set, the pattern applies if its body matches either the full path below `scope_dir` (treating the body as a path-shaped match when it contains an internal `/`) or any single path segment of that suffix (when the body contains no internal `/`).
2. Among all applying patterns the *last* one (deepest layer, then last in source order within the layer) wins. If its `is_negation` flag is set the path is *not* ignored by user rules; otherwise it *is* ignored by user rules.
3. If no user pattern applied, the path is not ignored by user rules.

Built-in excludes are then layered on:
- If any segment of `path` equals `.kitchensync`, the path is ignored regardless of user rules. This built-in cannot be negated.
- If the user-rule outcome is "not ignored" and the path's first segment is `.git` (i.e. `path == ".git"` or `path` starts with `.git/`) and no user pattern with `is_negation` set applied to it, the path is ignored. In other words, `.git/` behaves as an implicit deepest-priority ignore that any explicit user `!`-pattern can override.

`is_ignored_entry(m: Matcher, path: relative-path, kind: EntryKind) -> bool` — like `is_ignored`, but the caller supplies a filesystem entry kind. `EntryKind` is one of `file`, `dir`, `symlink`, `special`. For `symlink` and `special` the answer is always true regardless of `m`. For `file` the answer is `is_ignored(m, path, false)`. For `dir` the answer is `is_ignored(m, path, true)`.

### Introspection

`layer_count(m: Matcher) -> int` — the number of scope layers in `m` (zero for an empty matcher). Useful for tests and diagnostics.

`layer_at(m: Matcher, index: int) -> (scope_dir: relative-path, set: PatternSet)` — random access to the layer at the given zero-based index, with layer zero being the shallowest.

## Anchoring
- `Matcher`, `empty_matcher`, `push_scope`, layered evaluation, `is_ignored`, "last matching pattern wins" precedence across the stack, scope-relative interpretation of anchored and directory-only flags: the hierarchical-evaluation semantics described in the gitignore pattern syntax reference at git-scm.com, the recognised external standard for this format, and SPEC.md §"Hierarchy" and §"Querying the matcher".
- `PatternSet` and `CompiledPattern` (with their `is_negation`, `is_anchored`, `is_dir_only`, and source-order semantics): the structured form mandated by the gitignore pattern syntax reference. Any gitignore-conformant compiler produces values of this shape; this component consumes them as opaque inputs.
- Built-in always-ignored `.kitchensync` at any depth (non-negatable) and default-but-negatable `.git/` at the sync root: SPEC.md §"Built-in Excludes".
- `is_ignored_entry` and `EntryKind` (`file` / `dir` / `symlink` / `special`), with symlinks and special files always ignored: SPEC.md §"Built-in Excludes" (symlinks and special files) and the standard POSIX filesystem entry taxonomy.
- `relative-path` (forward-slash-separated, no leading slash, anchored at the sync root) and the boolean `is_dir` flag: host-language primitives and the standard POSIX relative-path convention.
- `layer_count`, `layer_at`: the standard immutable ordered-sequence abstraction over the stack, a host-language primitive.
