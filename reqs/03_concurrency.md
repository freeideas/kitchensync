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
