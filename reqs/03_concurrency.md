# Concurrency

Connection pools, fallback URL resolution, parallel operations, and pipelined transfers.

## $REQ_CONC_001: Connection Pool Per URL
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Connection pools are keyed by URL, not by peer. Each URL that successfully connects gets its own pool.

## $REQ_CONC_002: SFTP Pool Connections
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

For SFTP URLs, pool connections are real SSH/SFTP connections.

## $REQ_CONC_003: File URL Pool Connections
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

For `file://` URLs, connections are lightweight handles.

## $REQ_CONC_004: Max Connections Default
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

The default maximum connections per URL is 10, configurable via `max-connections`.

## $REQ_CONC_005: Connection Timeout Default
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

The default connection timeout is 30 seconds, configurable via `connection-timeout`.

## $REQ_CONC_006: Per-URL Settings Override Global
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

`max-connections` and `connection-timeout` can be set per-URL (config file only) or globally. Most specific wins.

## $REQ_CONC_007: Transfer Acquires Two Connections
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

A file transfer acquires one connection from the source peer's active URL pool and one from the destination peer's active URL pool. Both must be available before the transfer begins.

## $REQ_CONC_008: Connections Returned After Transfer
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

When a transfer completes or fails, both connections are returned to their respective pools.

## $REQ_CONC_009: Wait on Pool Exhaustion
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

If either pool is exhausted, the transfer waits until a connection is returned.

## $REQ_CONC_010: Lazy Pool Creation
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Connections are opened on demand up to the pool maximum, then recycled. The pool is created lazily.

## $REQ_CONC_011: Fallback URL Order
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

A peer's URLs are tried in order (primary first, then each fallback). The first successful connection wins; remaining URLs are not tried.

## $REQ_CONC_012: Connection Timeout for SFTP
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For SFTP URLs, `connection-timeout` bounds the SSH handshake. If it expires, the next URL is tried.

## $REQ_CONC_013: No Timeout for File URLs
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For `file://` URLs, `connection-timeout` does not apply.

## $REQ_CONC_014: Winning URL Determines Pool
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

The first successful connection's URL determines which pool is used for that peer's transfers for the remainder of the run.

## $REQ_CONC_015: All URLs Fail — Unreachable
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

If all URLs for a peer fail, the peer is unreachable for the entire run.

## $REQ_CONC_016: Directory Listing Connection
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

Directory listing uses its own connection per peer, outside the transfer pool.

## $REQ_CONC_017: Concurrent Directory Listings
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

During multi-tree traversal, directory listings for all reachable peers at each level are issued concurrently, not sequentially. Wall-clock time for listing one directory level should be approximately the time of the slowest peer.

## $REQ_CONC_018: Trace Log Pool Changes
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

When log level is `trace`, every pool acquire and release is logged with format `url=<url> connections=<n>/<max>`. This allows reconstruction of the concurrency timeline from the `applog` table.

## $REQ_CONC_019: Parallel Directory Listing Code Examination
**Source:** ./specs/concurrency.md (Section: "Parallel directory listing test")

A code examination test verifies that directory listings across peers are issued concurrently — not sequentially awaited. The code must use a concurrent join/gather/parallel construct rather than a sequential loop with individual awaits.

## $REQ_CONC_020: Pool Connection Failure Is Transfer Failure
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

A pool connection failure during a transfer is treated as a transfer failure — the transfer is logged as failed and skipped, not the entire run.

## $REQ_CONC_021: Startup Connection Per Peer
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

At startup, one connection per peer is established for directory listing. This initial connection triggers fallback URL resolution and determines the winning URL for that peer's transfers.

## $REQ_CONC_022: Pipelined Transfer Code Examination
**Source:** ./specs/concurrency.md (Section: "Pipelined transfer test")

A code examination test verifies that each file transfer uses two concurrent tasks connected by a bounded channel: one reading from the source, one writing to the destination. Reads and writes are not performed sequentially in a single loop.
