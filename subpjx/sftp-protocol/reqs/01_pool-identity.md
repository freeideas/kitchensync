# 01_pool-identity: Pool identity, keying, and URL-to-pool mapping

## Behavior

A pool is keyed by an `(sftp-user, host)` pair: every URL that resolves to the same user and host shares the same pool, regardless of path. The port is part of host identity for keying — distinct ports are distinct pool keys. The user is determined from the URL: an explicit username in the URL is used, otherwise the current OS user is used. Derives from `specs/SPEC.md` § "API surface > Pool".

## $REQ_IDs

- `01.1` — Two URLs with the same `(user, host)` but different paths resolve to the same pool.
- `01.2` — Two URLs with the same host but different ports resolve to different pools.
- `01.3` — A URL with an explicit username connects as that user.
- `01.4` — A URL with no username connects as the current OS user.
- `01.5` — Two simultaneous acquisitions for URLs in the same `(user, host)` pool both count against that pool's `max_connections`.

## Notes

- The path component of an SFTP URL plays no role in pool key resolution.
