# 02_path-hashing: Hash a relative path into a stable 11-character base62 identifier.

## Behavior
The component turns a relative path string (forward slashes, no leading or trailing slash) into an 11-character base62 identifier using xxHash64 with seed 0. Files and directories at the same relative path produce the same identifier — distinction between them lives in `byte_size`, not the id. A dedicated sentinel id, the hash of the literal string `/`, is the parent_id used for entries directly under the sync root. Derived from `SPEC.md` §"Identity and timestamp helpers".

## $REQ_IDs
- `02.1` — Hashing a relative path returns an identifier that is exactly 11 characters long.
- `02.2` — The identifier contains only base62 characters: digits `0-9`, uppercase `A-Z`, and lowercase `a-z`.
- `02.3` — Hashing the same relative path twice (in the same handle or across handles) returns the same identifier.
- `02.4` — The identifier is the xxHash64-seed-0 of the path bytes, encoded in base62 and zero-padded to 11 characters (verifiable against known-input/known-output vectors).
- `02.5` — The root parent sentinel equals the identifier produced by hashing the literal string `/`.

## Notes
Anchored by `database.md` §"Path Hashing".
