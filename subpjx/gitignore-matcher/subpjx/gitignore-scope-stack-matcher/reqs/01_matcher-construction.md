# 01_matcher-construction: Construct matchers by stacking scope layers

## Behavior
A `Matcher` is an immutable ordered stack of layers, each layer carrying a `scope_dir` and a `PatternSet`. Construction starts with `empty_matcher()` and grows by `push_scope(parent, scope_dir, set)`, which returns a new `Matcher` with one additional layer appended (parent unmodified). Layers are ordered shallowest-first. `layer_count` and `layer_at` expose the stack for introspection. Derived from SPEC.md §"Matcher value", §"Constructing matchers", and §"Introspection".

## $REQ_IDs
- `01.1` — `empty_matcher()` returns a `Matcher` whose `layer_count` is zero.
- `01.2` — `push_scope(parent, scope_dir, set)` returns a `Matcher` whose `layer_count` is `layer_count(parent) + 1`.
- `01.3` — `push_scope` does not mutate `parent`: after the call, `parent`'s `layer_count` and existing layer contents are unchanged.
- `01.4` — Layers in the returned matcher are ordered shallowest-first: the layer at index `layer_count(parent)` is the one just pushed.
- `01.5` — `layer_at(m, i)` returns the `(scope_dir, set)` recorded when that layer was pushed, with index 0 being the shallowest.
- `01.6` — `push_scope` with an empty `PatternSet` is accepted and produces a visible layer (`layer_count` increases and `layer_at` returns the pushed empty set).
- `01.7` — A `scope_dir` of the empty string is accepted and denotes the sync root.

## Notes
The `PatternSet` and `CompiledPattern` types are opaque inputs to this component (SPEC.md §"Anchoring"). Construction does not interpret pattern contents.
