# SnapshotIdentity:

## Purpose

SnapshotIdentity provides the deterministic identity and time values used by
SnapshotStore and its callers. It computes snapshot path IDs for rows below the
sync root, computes parent IDs for those rows, formats supplied UTC microsecond
times, and generates process-local UTC timestamp strings in the single format
used by snapshot columns, BAK directory names, TMP directory names, and log
output.

This child does not store snapshot rows. It gives other SnapshotStore behavior
the exact `id`, `parent_id`, `mod_time`, `last_seen`, `deleted_time`, and
caller-facing timestamp strings they must use.

## Responsibilities

SnapshotIdentity exposes an operation that returns the snapshot path ID for one
relative path below the sync root. The input path is the path relative to the
sync root using forward slash separators, no leading slash, and no trailing
slash. The same input rule is used for files and directories. The operation
hashes the input with xxHash64 seed 0 and returns the zero-padded
11-character base62 encoding of that `u64` value. Base62 uses exactly this
alphabet:

```text
0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz
```

SnapshotIdentity exposes an operation that returns the parent path ID for one
relative path below the sync root. For an entry whose relative path has no
slash, the operation returns `JyBskcNRrBK`, the path ID for `/`. For any deeper
entry, it applies the same path ID rule to the parent directory relative path.

SnapshotIdentity exposes the root parent ID constant `JyBskcNRrBK` for callers
that need to compare or bind the parent ID used by entries directly under the
sync root.

SnapshotIdentity exposes an operation that formats a caller-supplied UTC time
at microsecond precision. The operation returns a timestamp string in this exact
format and rejects values that cannot be represented in that format:

```text
YYYY-MM-DD_HH-mm-ss_ffffffZ
```

Formatted timestamp strings represent UTC time at microsecond precision and
sort lexicographically in the same order as their represented UTC times.
Snapshot row owners use this operation for observed modification times and for
copied deletion estimates that need to be carried as timestamp strings.

SnapshotIdentity exposes a process-local timestamp generator. Each call reads
the current UTC time, drops any sub-microsecond remainder, and compares that
UTC microsecond value with the last generated value in this process. If the
current value is not greater than the last generated value, the generator uses
the last generated value plus one microsecond. It formats the selected value
with the same timestamp operation and returns it. Callers that update
`last_seen` must call the generator separately for each snapshot row instead of
reusing a generated value across rows.

SnapshotIdentity rejects path ID inputs that do not follow the relative path
rule: empty strings, leading slashes, trailing slashes, repeated slash
separators, and `.` or `..` path components are invalid. Invalid path inputs
return an error and no path ID. Timestamp formatting rejects values outside the
representable range. Timestamp generation returns an error if the system clock
cannot be read or if the selected UTC value cannot be formatted, rather than
returning a malformed timestamp.

## Boundaries

SnapshotIdentity does not create, read, or update SQLite databases. It does not
decide whether the sync root itself should have a row; the row owner enforces
that no row is stored for the sync root and calls this child only for entries
below the root.

SnapshotIdentity does not parse operating system paths, normalize peer URLs, or
decide whether a discovered entry is a file or directory. Callers pass the
already chosen slash-separated relative path string.

SnapshotIdentity does not create BAK directories, create TMP directories, or
write log output. It only returns timestamp strings for those callers to use.

SnapshotIdentity does not generate `deleted_time` values for confirmed absence
or displacement. Row mutation behavior copies deletion estimates from existing
`last_seen` values where required.

## Invariants

- Path IDs are deterministic for the same valid relative path input.
- Path IDs are always 11 US-ASCII characters.
- Path IDs use xxHash64 seed 0.
- Path ID base62 output is left-padded with `0` to 11 characters.
- The base62 alphabet is
  `0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz`.
- File and directory path IDs use the same relative path input rule.
- Entries directly under the sync root use `JyBskcNRrBK` as `parent_id`.
- The sync root directory itself has no snapshot row.
- All timestamp strings returned by this child use
  `YYYY-MM-DD_HH-mm-ss_ffffffZ`.
- All timestamp strings returned by this child represent UTC microsecond
  values.
- Generated timestamps from one process are strictly increasing and can be
  sorted as plain strings to get UTC chronological order.
