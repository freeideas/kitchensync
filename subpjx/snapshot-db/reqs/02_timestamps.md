# 02_timestamps: Produce UTC microsecond timestamps that are filesystem-safe, sortable, and strictly monotonic within a handle.

## Behavior
A snapshot handle hands out the current UTC timestamp formatted as `YYYY-MM-DD_HH-mm-ss_ffffffZ` (microsecond precision). The format is filesystem-safe — only digits, dashes, underscores, and the trailing `Z` — and orders chronologically under plain string sort. Within one open handle the value is strictly monotonic: if the wall clock has not advanced between two requests, the second value is one microsecond after the first. Derived from `SPEC.md` §"Identity and timestamp helpers".

## $REQ_IDs
- `02.6` — A current-timestamp value matches the pattern `YYYY-MM-DD_HH-mm-ss_ffffffZ` (four-digit year, two-digit month/day/hour/minute/second, six-digit microseconds, trailing `Z`).
- `02.7` — A current-timestamp value, parsed under `YYYY-MM-DD_HH-mm-ss_ffffffZ` as UTC, falls at or after the system UTC clock captured immediately before the call and at or before the system UTC clock captured immediately after.
- `02.8` — Two timestamps produced by the same handle in the order A then B satisfy `A < B` under lexicographic string comparison.
- `02.9` — When two timestamps are requested back-to-back within the same wall-clock microsecond, the second value is exactly one microsecond after the first.

## Notes
Anchored by `database.md` §"Timestamps".
