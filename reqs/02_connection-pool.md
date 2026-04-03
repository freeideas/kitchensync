# Connection Pool and Concurrency

Connection pooling, concurrency limits, and pipelined transfers.

## $REQ_CONN_001: Pool Keyed by URL
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Connection pools are keyed by URL, not by peer. Each URL that successfully connects gets its own pool.

## $REQ_CONN_002: Lazy Pool Growth
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

The pool is lazy: connections open on demand up to the maximum, then recycle. Connections are reused across transfers.

## $REQ_CONN_003: Deadlock Prevention
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

File transfers acquire one connection from the source pool and one from the destination pool. To prevent deadlock, pools are always acquired in lexicographic order by normalized URL. If source and destination are the same pool, two connections are acquired from that pool.

## $REQ_CONN_004: Pool Acquisition Blocking
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Pool acquisition blocks until a connection is available; there is no acquisition timeout. On connection failure during transfer, the failed connection is removed from the pool and a new one is opened (up to the maximum).

## $REQ_CONN_005: Connection Timeout for SFTP
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For SFTP URLs, `--ct` (default: 30 seconds) bounds the SSH handshake. After handshake, the peer's root path is verified/created. `file://` URLs verify/create the local path; timeout does not apply.

## $REQ_CONN_006: Winning URL Used for All Connections
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

When fallback URLs are tried, the first successful connection wins. All subsequent connections for that peer use the winning URL's pool.

## $REQ_CONN_007: Directory Listing Connection
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

Directory listing uses its own connection per peer, outside the transfer pool. Inline operations during the walk (displace, create_dir, .syncignore reads) reuse this listing connection.

## $REQ_CONN_008: Concurrent Directory Listings
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

During multi-tree traversal, directory listings for all reachable peers at each level are issued concurrently. Wall-clock time for one level is approximately the slowest peer.

## $REQ_CONN_009: Pipelined Transfers
**Source:** ./specs/concurrency.md (Section: "Pipelined Transfers")

Each file transfer uses two goroutines connected by a buffered channel: a reader that reads chunks from the source and a writer that writes to the destination. Reader and writer operate simultaneously with backpressure from the channel.

## $REQ_CONN_010: SSH Host Key Algorithm Constraint
**Source:** ./README.md (Section: "Authentication")

KitchenSync reads `~/.ssh/known_hosts` to determine which key algorithms are recorded for each host and constrains the SSH handshake to negotiate only key types that `known_hosts` can verify.

## $REQ_CONN_011: Per-URL Settings Override Global
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Per-URL settings specified as query string parameters override global flag settings. For example, `sftp://user@host/path?mc=20&ct=60` overrides `--mc` and `--ct` for that URL.

## $REQ_CONN_014: OS Hostname Resolution for SFTP
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

SFTP connections must use OS hostname resolution. Bare hostnames like `localhost` must resolve correctly.

## $REQ_CONN_012: Trace Logging of Pool Changes
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace` (`-vl trace`), every pool acquire and release is logged showing the URL and current/max connection counts (e.g., `url=sftp://user@host/path connections=2/10`).

## $REQ_CONN_013: Trace Logging of Pipelined Transfers
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace`, pipelined transfer goroutine lifecycle is logged. Each goroutine logs `reader-start`/`reader-done` and `writer-start`/`writer-done` with source/destination and file path. Concurrent operation is confirmed when `writer-start` appears before `reader-done` for the same file.
