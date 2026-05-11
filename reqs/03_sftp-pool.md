# 03_sftp-pool: SFTP connection pool and per-URL tuning

## Behavior

SFTP connections are pooled per user+host (the path component is not part of the pool key). `--mc` caps concurrent connections in a pool, `--ka` keeps idle connections alive, `--ct` bounds SSH handshakes. Per-URL query-string settings override the corresponding global flag for that URL. `file://` peers have no pool. Derived from `specs/concurrency.md` §"Connection Pool (SFTP)" and `specs/sync.md` §"Per-URL Settings".

## $REQ_IDs
- `03.21` — Two SFTP URLs that share the same user+host use a single shared connection pool; the path component is not part of the pool key.
- `03.22` — A pool's max-open-connections cap defaults to 10 (overridable by `--mc`).
- `03.23` — A returned idle SFTP connection remains reusable from the pool for up to `--ka` seconds (default 30).
- `03.24` — When `--ka` seconds elapse with no reuse, the next acquire from the pool opens a new SSH+SFTP session rather than reusing the previous idle one.
- `03.25` — A file transfer acquires one connection from the source peer's pool and one from the destination peer's pool before the transfer begins.
- `03.26` — Per-URL query-string settings (`mc`, `ct`, `ka`) override the corresponding global flag for that URL.
- `03.27` — When a pool's max-open cap is reached, a caller requesting another connection from that pool waits until one is returned.
- `03.28` — `file://` peers use no connection pool; no pool acquire/release events are emitted for them at any verbosity level.
- `03.29` — Reusing a returned idle connection within the keep-alive window resets that connection's keep-alive timer.
- `03.30` — Both pool connections used by a transfer are returned to their pools when the transfer completes or fails.
