# Concurrency

## Connection Pool

Connection pools are keyed by URL, not by peer. Each URL that successfully connects gets its own pool. For SFTP URLs, these are SSH/SFTP connections. For `file://` URLs, connections are lightweight handles. The `--mc` pool limit applies equally to both.

| Setting            | Default | Global flag | Per-URL query param |
| ------------------ | ------- | ----------- | ------------------- |
| Max connections    | 10      | `--mc`      | `mc`                |
| Connection timeout | 30s     | `--ct`      | `ct`                |

Per-URL settings (query string) override global settings. Example: `"sftp://host/path?mc=20&ct=60"`

A file transfer acquires one connection from the source pool and one from the destination pool for the duration. Both must be available before the transfer begins. On completion or failure, both are returned.

Connections are reused across transfers. The pool is lazy: connections open on demand up to the maximum, then recycle.

## Fallback URLs

A peer can have multiple URLs in square brackets — fallback network paths to the same data. Tried in order; first that connects wins.

```
kitchensync [sftp://192.168.1.50/photos,sftp://nas.vpn/photos] /local/photos
```

The `+`/`-` prefix goes on the bracket, not on individual URLs.

## Connection Establishment

Every connection — directory listing or transfer pool — follows the same procedure:

1. Try the peer's primary URL first, then each fallback URL in order
2. For SFTP URLs, `--ct` (default: 30s) bounds the SSH handshake. After handshake, verify/create the peer's root path. If creation fails, try next fallback
3. For `file://` URLs, verify/create the local path. Timeout does not apply
4. First success wins. All subsequent connections use the winning URL's pool
5. If all URLs fail, the peer is unreachable

SFTP connections must use OS hostname resolution (Go's `net.Dial`). Bare hostnames like `localhost` must resolve correctly.

## Directory Listing

Directory listing uses its own connection per peer, outside the transfer pool. During multi-tree traversal, directory listings for all reachable peers at each level are issued concurrently (goroutines). With N reachable peers, wall-clock time for one level is approximately the slowest peer, not the sum.

## Pipelined Transfers

Each file transfer uses two goroutines connected by a buffered channel: a reader goroutine that reads chunks from the source and sends them into the channel, and a writer goroutine that receives chunks and writes to the destination. Reader and writer operate simultaneously — the channel provides backpressure. A single-loop read-then-write pattern is not acceptable.

## Trace Logging

At verbosity `trace` (`-vl trace`), log every pool change:

```
url=sftp://host/path connections=2/10
```

Logged on every acquire and release.
