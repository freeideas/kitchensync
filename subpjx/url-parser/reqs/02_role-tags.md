# 02_role-tags: Optional leading role tag drives the group role

## Behavior

A tagged URL group may begin with a single-character role tag. Three tag values exist (`+`, `-`, none) and each maps to a distinct opaque label on the returned `TaggedGroup.role`. The role tag appears at most once and applies to the entire group. Derived from SPEC.md section "Grammar" (role tag table) and "API surface" (rejection list).

## $REQ_IDs

- `02.1` — Absence of a leading tag produces `TaggedGroup.role = Normal`.
- `02.2` — A leading `+` produces `TaggedGroup.role = Canon`.
- `02.3` — A leading `-` produces `TaggedGroup.role = Subordinate`.
- `02.4` — Input carrying more than one role tag is rejected.

## Notes

`Canon`, `Subordinate`, and `Normal` are opaque labels; the parser assigns no behavioural meaning beyond distinguishing the three values.
