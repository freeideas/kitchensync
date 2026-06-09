# Identity:

## Purpose

Identity turns a tracked entry's relative path into the stable string that names
its row in the snapshot database, and into the string that names its parent's
row. It is a pure, dependency-free primitive: the same path always produces the
same identity, with no I/O, no clock, and no shared mutable state. Store reuses
it to compute row ids and parent links, so the hashing rule lives in exactly one
place and stays uniform across the whole run.

The rule is fixed: an identity is the xxHash64 (seed 0) of a canonical relative
path, base62-encoded and zero-padded to 11 characters. Because files and
directories with the same path hash identically, the byte size column (not the
identity) is what later distinguishes a directory from a file.

## Responsibilities

The operations Identity exposes across its boundary:

- Compute the identity of an entry from its relative path. The path is first put
  into canonical form, then hashed with xxHash64 using seed 0 (014.1), and the
  64-bit result is base62-encoded using digits `0-9`, then uppercase `A-Z`, then
  lowercase `a-z` (014.2), producing a zero-padded 11-character string (014.3).

- Compute the parent identity of an entry from its relative path. This is the
  identity of the path's parent directory: the same hash applied to the canonical
  path with its last segment removed. A root-level entry, whose canonical path has
  no parent segment, takes the identity of the sentinel path `/` (014.12).

Canonicalization, applied before hashing in both operations:

- Use forward slashes as the separator between segments (014.4).
- Remove any leading slash (014.5).
- Remove any trailing slash (014.6).
- Apply the same canonical form to a file and a directory, so an entry's type
  never affects its identity (014.7).

Worked examples that are part of the observable behavior:

- The identity of `docs/readme.txt` is the hash of `docs/readme.txt` (014.8).
- The identity of the directory `docs/notes` is the hash of `docs/notes` (014.9).
- The parent identity of `docs/readme.txt` is the hash of `docs` (014.10).
- The parent identity of the directory `docs/notes` is the hash of `docs`
  (014.11).

## Boundaries

Error obligations:

- Identity does no I/O and reaches no filesystem, so it raises no transport or
  database errors. It is given a relative path that the caller has already chosen
  to track; it does not validate that the path exists or decide whether it should
  be tracked.

Invariants:

- The computation is pure and deterministic: identical canonical paths always
  yield identical identities, and a file and a directory sharing a canonical path
  share an identity (014.7).
- Every identity it returns is exactly 11 base62 characters, zero-padded on the
  left (014.3).
- Canonicalization always yields forward slashes with no leading or trailing
  slash before the hash is taken (014.4, 014.5, 014.6).
- The parent of a root-level entry is always the hash of the sentinel `/`, never
  the hash of an empty string (014.12).

What Identity does not do:

- It does not own or open the snapshot database; it only supplies the id and
  parent_id values that Store writes into rows. The columns that store these
  hashes belong to the schema concern, not to Identity.
- It does not track the sync root directory itself. The sync root has no row;
  only its children are tracked, and Identity is asked only for those children's
  paths (014.13). It never produces an identity for the root as a tracked entry,
  though its sentinel `/` is what root-level children name as their parent.
- It does not normalize or hash URLs; peer-URL identity is a separate concern.
- It does not generate timestamps, classify entries, or apply any sync decision
  rule.
