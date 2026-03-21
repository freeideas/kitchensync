# Concurrency

## Connection Pool

Connection pools are keyed by URL, not by peer. Each URL that successfully connects gets its own pool of connections. For SFTP URLs, these are real SSH/SFTP connections. For `file://` URLs, connections are lightweight handles.

| Limit              | Default | Config key           |
| ------------------ | ------- | -------------------- |
| Max connections    | 10      | `max-connections`    |
| Connection timeout | 30      | `connection-timeout` |

Both settings can be set at two levels (most specific wins):

1. **Per-URL** — on a URL object in the peer's fallback URL list (config file only)
2. **Global default** — in the config root or on the command line

A file transfer from peer A to peer B acquires one connection from A's active URL pool and one from B's active URL pool for the duration of the transfer. Both connections must be available before the transfer begins. When the transfer completes (or fails), both connections are returned to their respective pools.

If either pool is exhausted, the transfer waits until a connection is returned.

Connections are reused across transfers. The pool is created lazily: connections are opened on demand up to the pool maximum, and then recycled.

## Fallback URLs

A peer can have multiple URLs in its `urls` list — these are fallback network paths to the same data. They share one `peer_id` and one set of snapshot rows. URLs are tried in order; the first that connects wins.

```json5
{
  "peer_groups": [
    {
      "name": "docs",
      "peers": [
        { "name": "local", "urls": ["file:///home/user/docs"] },
        { "name": "nas", "urls": [
            { "url": "sftp://bilbo@192.168.1.50/docs", "max-connections": 20 },
            { "url": "sftp://bilbo@nas.vpn/docs", "max-connections": 3, "connection-timeout": 60 }
          ]
        }
      ]
    }
  ]
}
```

On the CLI, each URL argument is a separate peer (single URL). Multiple fallback URLs per peer are a config-file feature.

## Connection Establishment

Every connection to a peer — whether for directory listing or from the transfer pool — is established the same way:

1. Try the peer's primary URL first, then each fallback URL in order
2. For SFTP URLs, `connection-timeout` (default: 30 seconds) bounds the SSH handshake; if it expires, try the next URL
3. For `file://` URLs, the connection is a lightweight local handle — `connection-timeout` does not apply
4. First successful connection wins — remaining URLs are not tried. All subsequent connections for this peer use the winning URL's pool
5. If all URLs fail, the peer is **unreachable** for this connection attempt

At startup, one connection per peer is established for directory listing (see below). The winning URL determines which pool is used for that peer's transfers for the remainder of the run. If all URLs fail, the peer is unreachable for the entire run (see sync.md startup step 7). Pool connections are opened lazily using the same procedure; a pool connection failure during a transfer is a transfer failure (see sync.md Errors).

## Directory Listing

Directory listing uses its own connection per peer, outside the transfer pool. During multi-tree traversal, directory listings for all reachable peers at each level must be issued concurrently, not sequentially. With N reachable peers, the wall-clock time for listing one directory level should be approximately the time of the slowest peer, not the sum of all peers.

## Trace Logging

When log level is `trace`, log every pool change:

```
url=<url> connections=<n>/<max>
```

Logged on every acquire and release. This allows tests to reconstruct the concurrency timeline from the `applog` table and verify that limits were never exceeded.

## Parallel directory listing test

This is a code examination test, not a runtime test. The test reads the source files that implement the multi-tree traversal and verifies that directory listings across peers are issued concurrently. Specifically, it must confirm that the code does not list peers sequentially (e.g., awaiting each peer's listing before starting the next). Look for patterns such as: all peer listings collected into a concurrent join/gather/parallel construct rather than a sequential loop with individual awaits.

## Pipelined transfer test

This is a code examination test, not a runtime test. The test reads the source files that implement file transfers and verifies that each transfer uses two concurrent tasks connected by a bounded channel: one task reading from the source peer and one task writing to the destination peer. Specifically, it must confirm that reads and writes are not performed sequentially in a single loop (e.g., read a chunk then write it, then read the next). Look for patterns such as: two spawned tasks or futures, a bounded channel connecting them, and concurrent execution (join/gather/select) rather than alternating read-write in one task.
