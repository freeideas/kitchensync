# 04_subordinates: Subordinate participants receive outcomes without voting

## Behavior
Participants with the `subordinate` role do not contribute to the decision; they are reconciled to the chosen state alongside contributing participants. When no contributing participant votes for the entry's existence, subordinates still holding the entry are displaced. Derived from SPEC.md "Inputs" → "Roles" and the closing paragraph of "Decision rules".

## $REQ_IDs
- `04.7` — A subordinate participant's observation does not influence the voting outcome (the decision is identical to the same scenario with the subordinate omitted).
- `04.8` — When `entry_kind` is `None`, subordinate participants holding the entry get `Displace`.
