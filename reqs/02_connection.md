# Connection Management

Connection establishment, connection pooling, and concurrency control.

## $REQ_CONN_001: URL Fallback Order
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

When establishing a connection to a peer, URLs in the peer's `urls` list are tried top to bottom. The first successful connection wins; remaining URLs are not tried.

## $REQ_CONN_002: SFTP Connection Timeout
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For SFTP URLs, the `connection-timeout` setting (default: 30 seconds) bounds the SSH handshake. If it expires, the next URL is tried.

## $REQ_CONN_003: File URL No Timeout
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For `file://` URLs, the connection is a lightweight local handle — `connection-timeout` does not apply.

## $REQ_CONN_004: Unreachable Peer
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

If all URLs for a peer fail, the peer is unreachable for that connection attempt.

## $REQ_CONN_005: Connection Pool Per Peer
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Each peer maintains a pool of connections. The maximum pool size is controlled by `max-connections` (default: 10), a global default in the config root.

## $REQ_CONN_006: Dual Connection Acquisition
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

A file transfer acquires one connection from the source peer's pool and one from the destination peer's pool. Both connections must be available before the transfer begins. When the transfer completes or fails, both connections are returned to their respective pools.

## $REQ_CONN_007: Pool Exhaustion Wait
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

If either pool is exhausted, the transfer waits until a connection is returned.

## $REQ_CONN_008: Lazy Pool Creation
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Connections are opened on demand up to the pool maximum, then recycled. The pool is created lazily.

## $REQ_CONN_009: Separate Directory Listing Connection
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

Directory listing uses its own connection per peer, outside the transfer pool.

## $REQ_CONN_010: Concurrent Directory Listing
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

During multi-tree traversal, directory listings for all reachable peers at each level are issued concurrently. With N reachable peers, the wall-clock time for listing one directory level should be approximately the time of the slowest peer, not the sum of all peers.

## $REQ_CONN_011: Trace Logging of Pool Changes
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

When log level is `trace`, every pool acquire and release is logged in the format `peer=<name> connections=<n>/<max>`. This allows reconstruction of the concurrency timeline from the `applog` table to verify that limits were never exceeded.

## $REQ_CONN_012: Parallel Directory Listing Code Structure
**Source:** ./specs/concurrency.md (Section: "Parallel directory listing test")

The source code that implements multi-tree traversal must issue directory listings across peers concurrently — using a concurrent join/gather/parallel construct rather than a sequential loop with individual awaits. This is verified by code examination.

## $REQ_CONN_013: Pipelined Transfer Code Structure
**Source:** ./specs/concurrency.md (Section: "Pipelined transfer test")

The source code that implements file transfers must use two concurrent tasks connected by a bounded channel: one reading from the source peer and one writing to the destination peer. Reads and writes must not be performed sequentially in a single loop. This is verified by code examination.

## $REQ_CONN_014: Startup Connection
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

At startup, one connection per peer is established for directory listing. If all URLs fail for a peer, it is unreachable for the entire run.
