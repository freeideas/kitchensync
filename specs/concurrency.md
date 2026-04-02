# Concurrency

## Connection Pool

Connection pools are keyed by URL, not by peer. Each URL that successfully connects gets its own pool. For SFTP URLs, these are SSH/SFTP connections. For `file://` URLs, connections are lightweight handles. The `--mc` pool limit applies equally to both.

| Setting            | Default | Global flag | Per-URL query param |
| ------------------ | ------- | ----------- | ------------------- |
| Max connections    | 10      | `--mc`      | `mc`                |
| Connection timeout | 30s     | `--ct`      | `ct`                |

Per-URL settings (query string) override global settings. Example: `"sftp://user@host/path?mc=20&ct=60"`

A file transfer acquires one connection from the source pool and one from the destination pool for the duration. To prevent deadlock, always acquire the two pools in lexicographic order by URL (the normalized URL that keys the pool). If source and destination are the same pool (local-to-local copy), acquire two connections from that pool. On completion or failure, both are returned.

Connections are reused across transfers. The pool is lazy: connections open on demand up to the maximum, then recycle.

Pool acquisition blocks until a connection is available; there is no acquisition timeout. This is acceptable because the transfer queue has finite size and connections are always returned. If a connection fails during transfer, it is removed from the pool and a new one is opened (up to the maximum) for subsequent transfers.

## Fallback URLs

A peer can have multiple URLs in square brackets — fallback network paths to the same data. Tried in order; first that connects wins.

```
kitchensync [sftp://user@192.168.1.50/photos,sftp://user@nas.vpn/photos] /local/photos
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

Directory listing uses its own connection per peer, outside the transfer pool. Inline operations during the walk (displace, create_dir, .syncignore reads) reuse this listing connection — they do not acquire from the transfer pool. During multi-tree traversal, directory listings for all reachable peers at each level are issued concurrently (goroutines). With N reachable peers, wall-clock time for one level is approximately the slowest peer, not the sum.

## Pipelined Transfers

Each file transfer uses two goroutines connected by a buffered channel: a reader goroutine that reads chunks from the source and sends them into the channel, and a writer goroutine that receives chunks and writes to the destination. Reader and writer operate simultaneously — the channel provides backpressure. A single-loop read-then-write pattern is not acceptable.

Recommended chunk size: 64KB. Channel buffer: 16 chunks. These are implementation hints, not requirements.

## Trace Logging

At verbosity `trace` (`-vl trace`), log every pool change:

```
url=sftp://user@host/path connections=2/10
```

Logged on every acquire and release.

Also at `trace`, log pipelined transfer goroutine lifecycle:

```
pipe reader-start src=sftp://user@host/path file=photos/img.jpg
pipe writer-start dst=/local/photos file=photos/img.jpg
pipe reader-done  src=sftp://user@host/path file=photos/img.jpg
pipe writer-done  dst=/local/photos file=photos/img.jpg
```

Each goroutine logs `*-start` before its first I/O and `*-done` after its last. Concurrent operation is confirmed when `writer-start` appears before `reader-done` for the same file.
