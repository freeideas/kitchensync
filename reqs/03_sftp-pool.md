# 03_sftp-pool: SFTP connection pool

## Behavior

SFTP connections are pooled per user+host. Each pool obeys a max-connection cap (`mc`), an SSH handshake timeout (`ct`), and an idle keep-alive TTL (`ka`); per-URL query parameters override the global flags. Local `file://` peers do not use a pool. Derived from `concurrency.md` §"Connection Pool (SFTP)" / §"Connection Establishment" and `sync.md` §"Per-URL Settings".

## $REQ_IDs

- `03.58` — Pool identity is the SFTP URL's user+host pair: two SFTP URLs that share the same user+host share a single pool, even if their path components differ.
- `03.59` — Per-URL `mc`, `ct`, and `ka` query parameters override the corresponding global flags (`--mc`, `--ct`, `--ka`) for that URL.
- `03.60` — A pool will not hold more than `mc` open connections at once; when all are busy, additional callers wait until a connection is returned.
- `03.61` — A returned connection stays alive in the pool for up to `ka` seconds and is reused if requested within that window.
- `03.62` — An SSH handshake that does not complete within `ct` seconds for an SFTP URL is treated as a failed connection (and the next fallback URL, if any, is tried).
- `03.63` — `file://` peers do not allocate a connection pool, and the `--mc`/`--ct`/`--ka` flags have no effect on them.
- `03.64` — A file transfer borrows one connection from the source peer's pool and one from the destination peer's pool for the transfer's duration; both connections are returned to their pools when the transfer completes or fails.
- `03.96` — SFTP pool identity includes normalized `user@host:port` (including port), so URLs to the same endpoint share one pool and URLs to different ports do not.
- `03.97` — For a shared SFTP pool endpoint, the first winning URL sets the pool's `mc` and `ka` settings; later URLs for that same endpoint do not override them.
- `03.100` — An SFTP URL with an explicit non-default port connects to that SSH port; an omitted/default port connects to port 22.
- `03.101` — Enqueued file copies execute concurrently when their required source and destination connections are available, subject to per-peer `mc` limits.
- `03.106` — Reusing an idle SFTP connection within its `ka` window resets that connection's keep-alive timer.
- `03.107` — When multiple peer arguments select the same SFTP endpoint with different `mc` or `ka` values, the earliest peer argument in command-line order supplies that endpoint pool's `mc` and `ka` settings.
- `03.112` — Startup SFTP connection attempts are not borrowed from the file-transfer pool and do not count against the endpoint's `mc` transfer-connection limit.
- `03.114` — An SFTP transfer pool and its transfer connections are created lazily; the first successful transfer connection to a `user@host:port` endpoint creates that endpoint's pool.

## Notes

Trace-level pool acquire/release logging is in `03_logging.md`.
