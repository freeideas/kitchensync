# 015_timestamps: Timestamp format and generation

## Behavior
This concern derives from `specs/database.md` section "Timestamps".

It covers the single timestamp string format used everywhere timestamps appear -
database columns, BAK/ and TMP/ directory names, and log output:
`YYYY-MM-DD_HH-mm-ss_ffffffZ`, UTC, microsecond precision, lexicographically
sortable and filesystem-safe. It covers the generation rule: every call site
that needs a fresh "now" (each `last_seen` write, each BAK/ or TMP/ directory
name) calls the generator and receives a value strictly greater than any it
returned before in the process (add one microsecond on collision), so a single
run never reuses one timestamp. It covers the exception that `deleted_time` is a
copied deletion estimate (taken from an existing `last_seen`, reused across
descendant cascades) and is exempt from the uniqueness rule.

The columns that store timestamps are `013_snapshot-schema`. The directory names
that embed timestamps are `021_staging-and-displacement`.

## $REQ_IDs

- `015.1` -- Every timestamp value matches the format `YYYY-MM-DD_HH-mm-ss_ffffffZ`.
- `015.2` -- Every timestamp value is expressed in UTC and ends with `Z`.
- `015.3` -- Every timestamp value carries microsecond precision (six fractional-second digits).
- `015.4` -- Sorting timestamp values as plain strings orders them chronologically.
- `015.5` -- The same timestamp format is used for database timestamp columns, BAK/ directory names, TMP/ directory names, and log output.
- `015.6` -- Each `last_seen` value set during a sync run is a freshly generated timestamp.
- `015.7` -- Each BAK/ or TMP/ directory created during a sync run is named with a freshly generated timestamp.
- `015.8` -- Within a single sync run, no two freshly generated timestamps are equal.
- `015.9` -- `deleted_time` is set from the row's existing `last_seen` value rather than a freshly generated timestamp.
- `015.10` -- In a descendant cascade, affected descendant rows receive the displaced entry's `deleted_time` value.

## Notes

Bullets 015.6, 015.7, 015.9, and 015.10 touch sites owned by other
categories (`013_snapshot-schema` columns, `021_staging-and-displacement`
directory names, tombstone cascades). They are kept here because the spec's
"Timestamps" section defines the generation-and-exemption behavior at those
sites; the column definitions and directory-naming formats themselves remain
with their owning categories.
