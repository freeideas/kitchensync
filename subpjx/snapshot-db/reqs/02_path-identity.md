# 02_path-identity: identify() maps paths to 11-character base62 identities

## Behavior
`identify(relative_path)` deterministically maps a forward-slash-delimited UTF-8 relative path to an 11-character base62 string by hashing the input with xxHash64 (seed 0) and zero-padding the base62 encoding. Files and directories at the same path string share an identity; the empty string and `/` both denote a single root-sentinel identity used as `parent_id` for top-level entries. Identities are portable — same input string yields the same output across calls, processes, machines, and runs. Derived from `./specs/SPEC.md` § "Path identity".

## $REQ_IDs
- `02.1` — `identify(p)` returns a string exactly 11 characters long for any valid input.
- `02.2` — The returned string contains only characters drawn from `0-9`, `A-Z`, `a-z`.
- `02.3` — `identify("")` and `identify("/")` return the same value (the root-sentinel identity).
- `02.4` — `identify(p)` returns the same value on every call with the same input `p` within and across runs.
- `02.5` — `identify(p)` does not depend on whether `p` refers to a file or a directory — the function is purely over the path string.
- `02.6` — For documented inputs, `identify(p)` equals the base62 zero-padded-to-11 encoding of xxHash64(`p` as UTF-8 bytes, seed=0) using the alphabet `0-9 A-Z a-z` in that order.

## Notes
Bullet 02.6 is the strongest portability check available within a single test process — it verifies the documented hash algorithm and encoding against fixed reference vectors.
