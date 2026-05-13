# 03_voting-no-canon: Voting rules when no canon participant is present

## Behavior
Without a canon participant, only `contributing` participants vote. Among live observations, the newest `mod_time` wins (with tolerance treating near-equal times as tied); when times are tied, `byte_size` resolves the remaining ambiguity. Derived from SPEC.md "Decision rules" → "Without a canon participant" rules 1, 2, 3, 5.

## $REQ_IDs
- `03.9` — When every contributing participant is classified `Unchanged`, every action is `NoOp`.
- `03.10` — Among contributing participants with a live observation, the participant with the newest `mod_time` provides the winning file metadata.
- `03.11` — Any live observer whose `mod_time` is within tolerance of the maximum is treated as tied with it.
- `03.12` — When `mod_time` is tied (within tolerance) and `byte_size` differs, the larger `byte_size` wins.
- `03.15` — A `NoOpinion` participant's observation does not influence the voting outcome.
