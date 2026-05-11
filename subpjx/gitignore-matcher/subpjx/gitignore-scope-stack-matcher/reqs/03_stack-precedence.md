# 03_stack-precedence: Last matching pattern across the whole stack wins, with negation

## Behavior
Across stacked layers, `is_ignored` considers patterns in stack order (shallowest to deepest) and within each layer in source order. The last matching pattern across the entire stack is the decision: a non-negation pattern marks the path ignored by user rules; a negation pattern (`is_negation` set) marks it not ignored. If no user pattern applies, the path is not ignored by user rules. Each layer evaluates its anchoring against its own `scope_dir`. Derived from SPEC.md §"Querying" (evaluation rule, steps 1–3).

## $REQ_IDs
- `03.1` — With multiple stacked layers, patterns from every layer are considered during evaluation.
- `03.2` — Across layers, the deepest layer that has an applying pattern is the deciding layer.
- `03.3` — Within the deciding layer, the last applying pattern in source order is the decision.
- `03.4` — When the last applying pattern across the stack has `is_negation` not set, `is_ignored` returns true.
- `03.5` — When the last applying pattern across the stack has `is_negation` set, `is_ignored` returns false, overriding earlier non-negation matches.
- `03.6` — Each layer's anchoring and scope-suffix logic uses that layer's own `scope_dir`, not the scope of the deepest or any other layer.
