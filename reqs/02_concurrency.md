# Concurrency

Connection pools, fallback URL handling, concurrent directory listing, and pipelined file transfers.

## $REQ_CONC_001: Connection Pool Per URL
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Connection pools are keyed by URL, not by peer. Each URL that successfully connects gets its own pool of connections.

## $REQ_CONC_002: SFTP Pool Connections
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

For SFTP URLs, pool connections are real SSH/SFTP connections.

## $REQ_CONC_003: file:// Pool Connections
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

For `file://` URLs, connections are lightweight handles. The `--mc` pool limit applies equally to both schemes.

## $REQ_CONC_004: Per-URL Settings Override Global
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Per-URL settings (query string) override global settings. Example: `"sftp://host/path?mc=20&ct=60"`.

## $REQ_CONC_005: Transfer Acquires Two Connections
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

A file transfer acquires one connection from the source peer's active URL pool and one from the destination peer's active URL pool for the duration of the transfer. Both connections must be available before the transfer begins.

## $REQ_CONC_006: Connection Return After Transfer
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

When a transfer completes (or fails), both connections are returned to their respective pools.

## $REQ_CONC_007: Transfer Waits on Exhausted Pool
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

If either pool is exhausted, the transfer waits until a connection is returned.

## $REQ_CONC_008: Connection Reuse
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Connections are reused across transfers. The pool is created lazily: connections are opened on demand up to the pool maximum, and then recycled.

## $REQ_CONC_009: Fallback URL Order
**Source:** ./specs/concurrency.md (Section: "Fallback URLs")

A peer's fallback URLs (grouped in square brackets) are tried in order; the first that connects wins.

## $REQ_CONC_010: Per-URL Settings in Fallback Groups
**Source:** ./specs/concurrency.md (Section: "Fallback URLs")

Per-URL settings apply to individual URLs within a fallback group.

## $REQ_CONC_011: SFTP Connection Timeout
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For SFTP URLs, `--ct` (default: 30 seconds) bounds the SSH handshake. If it expires, the next fallback URL is tried.

## $REQ_CONC_012: SFTP Root Path Auto-Create
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

After an SFTP handshake succeeds, the peer's root path is checked on the remote server. If it does not exist, it is created (including any missing parents) via SFTP. If creation fails, the URL is treated as failed.

## $REQ_CONC_013: file:// No Timeout
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For `file://` URLs, connection timeout does not apply. If the local path does not exist, it is created (including any missing parents).

## $REQ_CONC_014: First Successful Connection Wins
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

The first successful connection wins — remaining URLs are not tried. All subsequent connections for that peer use the winning URL's pool.

## $REQ_CONC_015: Unreachable Peer on All URLs Failed
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

If all URLs fail for a peer, the peer is unreachable for the entire run.

## $REQ_CONC_016: Dedicated Directory Listing Connection
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

Directory listing uses its own connection per peer, outside the transfer pool.

## $REQ_CONC_017: Concurrent Directory Listing
**Source:** ./specs/concurrency.md (Section: "Directory Listing")

During multi-tree traversal, directory listings for all reachable peers at each level are issued concurrently, not sequentially. Wall-clock time for listing one directory level should approximate the time of the slowest peer, not the sum of all peers.

## $REQ_CONC_018: Trace Logging for Pool Changes
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

When verbosity level is `trace` (`-vl trace`), every pool acquire and release is logged with format: `url=<url> connections=<n>/<max>`.

## $REQ_CONC_019: Parallel Directory Listing Code Examination
**Source:** ./specs/concurrency.md (Section: "Parallel directory listing test")

Code examination: the source files implementing multi-tree traversal must issue directory listings concurrently across peers (e.g., concurrent join/gather/parallel construct) rather than sequentially awaiting each peer's listing.

## $REQ_CONC_020: Pipelined Transfer Code Examination
**Source:** ./specs/concurrency.md (Section: "Pipelined transfer test")

Code examination: the source files implementing file transfers must use two concurrent tasks connected by a bounded channel — one reading from the source and one writing to the destination — rather than a sequential read-then-write pattern in a single loop.
