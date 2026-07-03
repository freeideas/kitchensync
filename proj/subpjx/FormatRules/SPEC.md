# FormatRules:

## Purpose

FormatRules owns the shared text formats and timestamp comparisons that must be
identical across command parsing, peer startup, traversal, staging, snapshot
storage, cleanup, logging, and reconciliation.

This child exposes pure formatting, parsing, validation, identity, ID, and
comparison operations. It does not perform peer I/O, database I/O, command-line
parsing, or sync decisions. Other children use it so the same URL identity,
relative path, snapshot ID, metadata path, timestamp, deletion-estimate, and
five-second tolerance rules are applied everywhere.

## Responsibilities

FormatRules exposes peer URL identity normalization. Given one peer URL text and
the current working directory and OS username context, it returns the canonical
identity string used for peer comparison and lookup. Normalization must:

- Treat a peer argument with no URL scheme as a `file://` URL.
- Resolve `file://` paths to absolute paths from the supplied current working
  directory.
- Lowercase the URL scheme and hostname.
- Remove port `22` from SFTP URLs.
- Collapse consecutive slashes in the URL path.
- Remove a trailing slash from the URL path.
- Percent-decode unreserved characters only.
- Strip query-string parameters from the identity.
- Insert the current OS username into an SFTP URL that has no username.

Query-string settings may be parsed by command-line or connection startup code,
but they are not part of the normalized identity returned by this child.
Normalization is a text and path operation only. It does not connect to a peer,
check whether a path exists, create roots, authenticate, or decide which
fallback URL wins.

FormatRules exposes validation for relative slash paths used by command-line
excludes, progress output, traversal, and snapshot path hashing. A valid user
tree path has slash separators, no leading slash, no trailing slash, no
backslash separator, no empty segment, no `.` segment, no `..` segment, and no
NUL character. The accepted path string is the exact relative path bytes used
for progress output and snapshot ID hashing.

Snapshot path hashing uses the same relative path bytes for files and
directories at the same path. The sync root directory itself has no snapshot
row. For a non-root entry, FormatRules exposes:

- The entry `id`: an 11-character, zero-padded base62 encoding of xxHash64
  seed 0 over the entry's full relative path bytes.
- The `parent_id`: the same encoding over the parent directory's relative path
  bytes.
- The root-entry `parent_id`: the same encoding over the sentinel bytes `/`.

Base62 IDs use this alphabet, in order: `0-9`, `A-Z`, `a-z`. The encoder must
always return exactly 11 characters by left-padding with `0`. The hashing
operation must hash the normalized slash path bytes directly, not a language
object representation.

FormatRules exposes timestamp parsing, formatting, and generation. Every
timestamp string written to snapshot columns, BAK directory names, TMP
directory names, and log output uses UTC `YYYY-MM-DD_HH-mm-ss_ffffffZ` format
with exactly six microsecond digits. The timestamp parser accepts only that
format. The formatter emits only that format.

Generated current timestamps are process-local and strictly increasing. Every
call that needs a new current timestamp for a `last_seen` write, BAK directory
name, or TMP directory name must receive a value greater than every generated
current timestamp already returned by this child in the same process. If the
clock does not advance, the generator advances the previous generated value by
one microsecond. Copied deletion estimates are not generated timestamps and do
not need to be unique.

FormatRules exposes deletion-estimate helpers for snapshot updates:

- A confirmed absence on a row whose `deleted_time` is NULL stores the row's
  existing `last_seen` as the new `deleted_time`.
- A repeated confirmed absence on a row whose `deleted_time` is already set
  leaves the existing `deleted_time` unchanged.
- A displacement to BAK stores that peer row's existing `last_seen` as the
  deletion estimate.
- A displacement cascade uses the displaced entry's copied deletion estimate
  for affected descendant rows on the same peer.

The helpers return the timestamp value to write or report that no write is
needed. They do not execute SQL and do not choose which rows are descendants.

FormatRules exposes metadata path formatting:

- BAK directory paths use `.kitchensync/BAK/<timestamp>/` at the affected parent
  directory, and cleanup age is determined from the `<timestamp>` path
  component.
- TMP directory paths use `.kitchensync/TMP/<timestamp>/`, and cleanup age is
  determined from the `<timestamp>` path component.
- User-entry SWAP paths use one percent-encoded basename path segment under
  `.kitchensync/SWAP/` for the target basename, with `new` and `old` children.
- Snapshot database SWAP paths are exactly
  `.kitchensync/SWAP/snapshot.db/new` and
  `.kitchensync/SWAP/snapshot.db/old`.

The user-entry SWAP basename encoder must produce one path segment on every
supported transport. It must not encode a whole relative path into one segment;
callers pass only the target basename.

FormatRules exposes five-second timestamp comparison helpers:

- File entry classification treats a current `mod_time` and snapshot row
  `mod_time` as the same when the absolute difference is no more than five
  seconds.
- The same values are different when the absolute difference is more than five
  seconds.
- Peer `mod_time` decisions treat any entry within five seconds of the maximum
  `mod_time` as tied with that maximum.
- A peer entry more than five seconds behind the maximum `mod_time` is older
  than the maximum.
- File deletion estimates win over existing file `mod_time` values only when
  the deletion estimate is more than five seconds newer than the file
  `mod_time`.
- An absent-unconfirmed file counts as a deletion only when its `last_seen`
  exceeds the maximum live-file `mod_time` by more than five seconds.
- A directory deletion estimate is newer than live-subtree file evidence only
  when it exceeds the newest live file `mod_time` by more than five seconds.
- Directory decision timestamp evidence ignores directory `mod_time` values.
- A live directory subtree with no files contributes no timestamp survival
  evidence.

Operations that parse or validate external text must return a clear validation
failure for malformed input instead of silently rewriting it into another
meaning. Invalid relative paths, invalid timestamp strings, invalid URL forms,
missing required context such as the OS username for an SFTP URL without a
username, and a SWAP basename that cannot be represented as one encoded segment
are errors crossing this boundary.

## Boundaries

FormatRules does not own the released CLI grammar, help text, validation error
wording, option defaults, or peer grouping. CommandLine decides when to call
relative path and URL validation and how to report command-line failures.

FormatRules does not own peer connection startup, fallback URL selection,
reachable-set rules, root creation, dry-run connection behavior, SFTP
authentication, host-key checking, or transport handles. Peer startup and
transport children use the normalized identity and path strings supplied by
this child.

FormatRules does not own the peer transport operation surface or any local or
SFTP filesystem operation. It only formats relative paths and metadata paths
that callers pass to transport operations.

FormatRules does not own the SQLite schema, transactions, snapshot download or
upload, SWAP recovery, stale-row cleanup, or recursive tombstone SQL. It owns
the ID strings, timestamp strings, and copied deletion-estimate values that
snapshot code stores.

FormatRules does not own reconciliation decisions. It exposes the exact
five-second comparison predicates and evidence filters so decision code can
apply the specified file and directory rules without duplicating time math.

Its invariants are:

- One input peer identity always normalizes to one deterministic identity
  string for the same current working directory and OS username context.
- Accepted relative user-tree paths are slash-separated, root-relative strings
  with no unsafe or empty segments.
- Snapshot IDs are deterministic 11-character base62 strings from xxHash64
  seed 0 over the specified bytes.
- The sync root itself never receives a snapshot row ID from this child.
- Timestamp strings accepted or emitted by this child use UTC with exactly six
  microsecond digits.
- Generated current timestamps are strictly increasing within the process.
- Copied deletion estimates preserve the source `last_seen` value and are not
  replaced with generated current time.
- All time comparisons that cross this boundary use the same five-second
  tolerance rule.
