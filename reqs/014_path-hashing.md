# 014_path-hashing: Path hashing and identity

## Behavior
This concern derives from `specs/database.md` section "Path Hashing".

It covers how a relative path is turned into a snapshot row identity: xxHash64
with seed 0, base62-encoded (digits, uppercase, lowercase) to a zero-padded
11-character string. It covers the canonical path form fed to the hash (forward
slashes, no leading or trailing slash, files and directories hashed identically),
the parent-id rule (hash of the parent directory's path, with root-level entries
using the hash of the sentinel `/`), and that the sync root itself is not tracked
- only its children are. The worked examples in that section are part of the
observable behavior.

The columns that store these hashes are `013_snapshot-schema`. URL identity
hashing/normalization is the separate concern `003_url-normalization`.

## $REQ_IDs

- `014.1` -- A snapshot entry's identity is the xxHash64 of its canonical relative path computed with seed 0.
- `014.2` -- The 64-bit hash is base62-encoded using digits `0-9`, then uppercase `A-Z`, then lowercase `a-z`.
- `014.3` -- The base62-encoded identity is a zero-padded 11-character string.
- `014.4` -- The canonical path fed to the hash uses forward slashes as separators.
- `014.5` -- The canonical path fed to the hash has no leading slash.
- `014.6` -- The canonical path fed to the hash has no trailing slash.
- `014.7` -- A file and a directory with the same canonical path produce the same identity.
- `014.8` -- The identity of `docs/readme.txt` is the hash of `docs/readme.txt`.
- `014.9` -- The identity of the directory `docs/notes` is the hash of `docs/notes`.
- `014.10` -- The parent identity of `docs/readme.txt` is the hash of `docs`.
- `014.11` -- The parent identity of the directory `docs/notes` is the hash of `docs`.
- `014.12` -- The parent identity of a root-level entry is the hash of the sentinel `/`.
- `014.13` -- The sync root directory has no snapshot row; only its children are tracked.
