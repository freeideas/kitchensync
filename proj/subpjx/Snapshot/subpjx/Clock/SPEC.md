# Clock:

## Purpose

Clock owns the single timestamp string format used everywhere a timestamp
appears in a run, and the one run-wide generator that hands out fresh "now"
values. It is a pure, dependency-free primitive: it does no I/O and reaches no
filesystem, but it does hold one piece of process-wide state, the highest value
it has handed out so far, so that every fresh value is strictly greater than the
last. Store reuses it for `last_seen` writes, and the sibling components that
name BAK/ and TMP/ directories and that write log output reuse the same format,
so the timestamp rule lives in exactly one place and stays uniform across the
whole run.

The format is fixed: `YYYY-MM-DD_HH-mm-ss_ffffffZ`, expressed in UTC, with
microsecond precision (six fractional-second digits) and a trailing `Z`. It is
chosen so that sorting timestamp values as plain strings orders them
chronologically, and so the value is safe to embed in a filesystem path.

## Responsibilities

The operations Clock exposes across its boundary:

- Produce a fresh "now" timestamp. Each call reads the current UTC time, formats
  it as `YYYY-MM-DD_HH-mm-ss_ffffffZ` (015.1, 015.2, 015.3), and returns a value
  strictly greater than every value it has returned before in this process. When
  the formatted time is not greater than the last value handed out, it advances
  by one microsecond and re-formats, repeating until the result is strictly
  greater (015.8). This is the value callers use for each `last_seen` write
  (015.6) and for each BAK/ or TMP/ directory name created during the run
  (015.7).

- Define the one format that every timestamp obeys. The same
  `YYYY-MM-DD_HH-mm-ss_ffffffZ` format is used for database timestamp columns,
  BAK/ directory names, TMP/ directory names, and log output (015.5). Because the
  fields run from most significant to least significant and every field is
  zero-padded to a fixed width, plain string sorting matches chronological order
  (015.4).

The deletion-estimate exception that Clock's uniqueness rule must accommodate:

- `deleted_time` is never a freshly generated timestamp. It is copied from a
  row's existing `last_seen` value (015.9), and in a descendant cascade the
  affected descendant rows receive the displaced entry's `deleted_time` value
  rather than fresh values (015.10). These copied values are exempt from the
  strictly-increasing uniqueness rule above; Clock's generator is asked only for
  the `last_seen` and directory-name sites, not for `deleted_time`.

## Boundaries

Error obligations:

- Clock does no I/O and reaches no filesystem or database, so it raises no
  transport or database errors. Its only failure surface is the formatting of the
  current time, which it does not expect to fail under normal operation.

Invariants:

- Every value Clock returns matches `YYYY-MM-DD_HH-mm-ss_ffffffZ`, is in UTC, and
  ends with `Z` (015.1, 015.2, 015.3).
- Within one run, every freshly generated value is strictly greater than every
  earlier one, so the values sort chronologically as plain strings and no two
  fresh values are ever equal (015.4, 015.8).
- The strictly-increasing state is process-wide: one source serves the entire run
  so that two callers asking at the same microsecond still receive distinct,
  ordered values.

What Clock does not do:

- It does not generate `deleted_time` values; those are copied from an existing
  `last_seen` and are exempt from its uniqueness rule (015.9, 015.10). Clock is
  asked only for fresh `last_seen` and directory-name timestamps.
- It does not own or open the snapshot database, write rows, or create BAK/ or
  TMP/ directories. It only supplies the string those callers store or embed; the
  columns and directory-naming sites belong to their owning concerns.
- It does not compute identities, classify entries, or apply any sync decision
  rule.
