# 03_concurrency: Per-URL connection pools, parallel listing, pipelined transfer

## Behavior

Connections to a peer are pooled per URL. The `--mc` global limit (and `?mc=` per-URL override) bounds the number of concurrent operations against any single URL. Directory listings across peers happen in parallel rather than sequentially, and each file transfer reads from source and writes to destination concurrently through a bounded channel. Derived from `./specs/concurrency.md` (`Connection Pool`, `Directory Listing`, `Parallel directory listing test`, `Pipelined transfer test`) and `./specs/sync.md` (`File Copy`).

## $REQ_IDs
- `03.71` — At most `--mc` concurrent operations execute against any single URL at one time; transfers wait for a connection when the pool is exhausted.
- `03.72` — A URL's `?mc=N` query-string parameter overrides `--mc` for that URL only.
- `03.73` — A file transfer between peers acquires one connection from the source URL's pool and one from the destination URL's pool while the transfer runs, returning both to their pools when it completes or fails.
- `03.74` — The `--mc` connection limit applies equally to `file://` URLs and `sftp://` URLs.
- `03.75` — Directory listings for all reachable peers at a given directory level are issued concurrently — wall-clock latency is dominated by the slowest peer, not the sum of all peers.
- `03.76` — Source code that implements multi-tree traversal collects per-peer listings via a parallel join/gather construct, not a sequential await-each-peer loop.
- `03.77` — Source code that implements file transfers spawns two concurrent tasks connected by a bounded channel — one reading from source, one writing to destination — rather than a single read-then-write loop.
