# 02_timestamps: now() returns monotonic UTC strings in a canonical format

## Behavior
`now()` returns the current UTC wall-clock time formatted as `YYYY-MM-DD_HH-mm-ss_ffffffZ` — zero-padded numeric fields joined by `-` and `_`, ending in a literal `Z`. The format is path-safe and lexicographically sortable in chronological order. The generator is process-monotonic: every returned value is strictly greater than every prior value returned by `now()` in the same process; when the wall clock has not advanced past the most recent value, the generator returns that value bumped by exactly one microsecond. Derived from `./specs/SPEC.md` § "Timestamps".

## $REQ_IDs
- `02.1` — `now()` returns a string matching the pattern `YYYY-MM-DD_HH-mm-ss_ffffffZ`, all numeric fields zero-padded to their stated widths.
- `02.2` — The numeric fields of `now()` correspond to the current UTC wall-clock time (not local time).
- `02.3` — Successive calls to `now()` within the same process return strictly increasing values.
- `02.4` — When `now()` is called faster than the wall clock advances, consecutive return values differ by exactly one microsecond.
