# 02_classification: Classify each participant by observation and history

## Behavior
For every participant the decision output reports a classification that compares the participant's current observation against its prior history record. Classifications are diagnostic and feed into the voting rules. Tolerance, a non-negative duration that defaults to 5 seconds, controls when two timestamps are considered equal. Derived from SPEC.md "Inputs" → "Tolerance" and "Classification".

## $REQ_IDs
- `02.1` — A participant observing `File` whose `mod_time` matches a non-tombstone history (within tolerance) is classified `Unchanged`.
- `02.2` — A participant observing `File` whose `mod_time` differs from a non-tombstone history is classified `Modified`.
- `02.3` — A participant observing `File` or `Directory` with a tombstone history (`deleted_time` non-null) is classified `Resurrected`.
- `02.4` — A participant observing `File` or `Directory` with no history record is classified `New`.
- `02.5` — A participant observing `Absent` with a tombstone history is classified `Deleted`.
- `02.6` — A participant observing `Absent` with a non-tombstone history (`deleted_time` null) is classified `AbsentUnconfirmed`.
- `02.7` — A participant observing `Absent` with no history record is classified `NoOpinion`.
- `02.8` — `mod_time` and `last_seen` comparisons treat two values as equal when they differ by at most the tolerance, which defaults to 5 seconds when not provided.
