# Concurrency

Connection pool management, deadlock prevention, and parallel operation execution.

## $REQ_CONC_001: Pools Keyed by URL
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Connection pools are keyed by URL, not by peer. Each URL that successfully connects gets its own pool.

## $REQ_CONC_002: Lazy Pool Growth
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

The pool is lazy: connections open on demand up to the maximum, then recycle. Connections are reused across transfers.

## $REQ_CONC_003: Deadlock Prevention by Lexicographic Order
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

To prevent deadlock, when a file transfer needs connections from two pools, the pools are acquired in lexicographic order by normalized URL.

## $REQ_CONC_004: Same Pool Acquires Two
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

If source and destination are the same pool (local-to-local copy), two connections are acquired from that single pool.

## $REQ_CONC_005: Listing Connection Separate from Pool
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

Directory listing uses its own connection per peer, outside the transfer pool. Inline operations during the walk (displace, create_dir, .syncignore reads) reuse this listing connection.

## $REQ_CONC_006: Concurrent Directory Listings
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

During the walk, directory listings for all reachable peers at each level are issued concurrently.

## $REQ_CONC_007: Max Connections Per URL
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

The `--mc` pool limit applies to each URL pool. Default is 10. Both SFTP and `file://` URLs are subject to the same limit.

## $REQ_CONC_008: Pool Acquisition Blocks
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Pool acquisition blocks until a connection is available; there is no acquisition timeout.

## $REQ_CONC_009: Failed Connection Replaced
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

If a connection fails during transfer, it is removed from the pool and a new one is opened (up to the maximum) for subsequent transfers.

## $REQ_CONC_010: Connection Timeout
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

The `--ct` flag sets connection timeout (default 30 seconds). For SFTP URLs, this bounds the SSH handshake.

## $REQ_CONC_011: Per-URL Settings Override Global
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Per-URL settings via query string (e.g., `?mc=20&ct=60`) override global flags.

## $REQ_CONC_012: Fallback URLs Tried in Order
**Source:** ./specs/concurrency.md (Section: "Fallback URLs"), ./specs/concurrency.md (Section: "Connection Establishment")

A peer can have multiple URLs in square brackets. They are tried in order; first that connects wins. All subsequent connections use the winning URL's pool.

## $REQ_CONC_013: Connection Establishment Verifies Root Path
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

After connection, the peer's root path is verified or created. If creation fails, the next fallback URL is tried.

## $REQ_CONC_019: File URL Timeout Does Not Apply
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For `file://` URLs, the connection timeout (`--ct`) does not apply.

## $REQ_CONC_020: Peer Unreachable When All URLs Fail
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

If all URLs (primary and fallbacks) fail to connect, the peer is unreachable.

## $REQ_CONC_014: SFTP Uses OS Hostname Resolution
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

SFTP connections must use OS hostname resolution. Bare hostnames like `localhost` must resolve correctly.

## $REQ_CONC_015: Pipelined Transfers with Concurrent Goroutines
**Source:** ./specs/concurrency.md (Section: "Pipelined Transfers")

Each file transfer uses a reader goroutine and a writer goroutine connected by a buffered channel. Reader and writer operate simultaneously. A single-loop read-then-write pattern is not acceptable.

## $REQ_CONC_016: Connections Returned After Transfer
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

On completion or failure, both source and destination connections are returned to their pools.

## $REQ_CONC_017: Pool Change Trace Logging
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace`, log every pool acquire and release in the format: `url=<url> connections=<current>/<max>`.

## $REQ_CONC_018: Pipeline Goroutine Trace Logging
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace`, log pipelined transfer goroutine lifecycle: `pipe reader-start`, `pipe writer-start`, `pipe reader-done`, `pipe writer-done` with source/destination and file path.
