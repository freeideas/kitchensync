# Concurrency

## Connection Pool (SFTP)

For `sftp://` URLs, connections are pooled by user+host. Each user+host pair that successfully connects gets its own pool of SSH+SFTP connections, owned by the SFTP transport (see `decomposition.md` §"sftp-protocol"). Two URLs that share the same user+host (e.g., `sftp://ace@host/foo` and `sftp://ace@host/bar`) share the same pool — a connection opened for one path can be reused for the other. The pool does not care about the path component of the URL.

| Setting              | Default | Global flag | Per-URL query param |
| -------------------- | ------- | ----------- | ------------------- |
| Max open connections | 10      | `--mc`      | `mc`                |
| Connection timeout   | 30s     | `--ct`      | `ct`                |
| Idle keep-alive TTL  | 30s     | `--ka`      | `ka`                |

Per-URL settings (query string) override global settings. Example: `"sftp://host/path?mc=20&ct=60&ka=10"`. The `ct` setting applies per connection attempt. The `mc` and `ka` settings configure the user+host pool; if two URLs that share the same user+host specify different `mc` or `ka` values, the resolution is implementation-defined (this is a misuse, not a supported configuration).

Pool semantics:

- **Open** — return an idle connection from the pool if one is available; otherwise open a new one (subject to `mc`). If `mc` connections are already open and all are busy, the caller waits until one is returned.
- **Close** — return the connection to the pool, where it remains alive for up to `ka` seconds. If reused within that window, the keep-alive timer resets. If not reused, the underlying SSH+SFTP session is actually closed when the timer expires.
- **Lifecycle** — connections are opened lazily and reused across operations. The pool is created lazily as well — first successful connect to a user+host pair creates its pool.

A file transfer from peer A to peer B borrows one connection from A's pool and one from B's pool for the duration of the transfer. Both connections must be available before the transfer begins. When the transfer completes (or fails), both connections are returned to their respective pools. (If A and B happen to share the same user+host, both connections come from the same pool.)

For `file://` URLs there is no connection pool — local file operations use the host language's standard library directly, and concurrency is bounded only by the OS's file descriptor limits and the host language's I/O scheduling. The `--mc`, `--ct`, and `--ka` flags have no effect on `file://` peers.

## Fallback URLs

A peer can have multiple URLs grouped in square brackets on the command line — these are fallback network paths to the same data. URLs are tried in order; the first that connects wins.

```
java -jar kitchensync.jar [sftp://192.168.1.50/photos,sftp://nas.vpn/photos] /local/photos
```

Per-URL settings apply to individual URLs within the group:

```
java -jar kitchensync.jar "[sftp://192.168.1.50/photos?mc=20,sftp://nas.vpn/photos?mc=3&ct=60]" /local/photos
```

## Connection Establishment

Every connection to a peer — whether for directory listing or from the transfer pool — is established the same way:

1. Try the peer's primary URL first, then each fallback URL in order
2. For SFTP URLs, `--ct` (default: 30 seconds) bounds the SSH handshake; if it expires, try the next URL. After the handshake succeeds, check whether the peer's root path exists on the remote server. If not, create it (and any missing parents) via SFTP. If creation fails, the URL is treated as failed (try next fallback)
3. For `file://` URLs, the connection is a lightweight local handle — connection timeout does not apply. If the local path does not exist, create it (and any missing parents) before connecting
4. First successful connection wins — remaining URLs are not tried. All subsequent connections for this peer use the pool keyed by the winning URL's user+host (for SFTP peers); `file://` peers do not use a pool
5. If all URLs fail, the peer is **unreachable** for this connection attempt

At startup, one connection per peer is established for directory listing (see below). For SFTP peers, the winning URL's user+host determines which pool that peer uses for its transfers for the remainder of the run. If all URLs fail, the peer is unreachable for the entire run (see sync.md startup). Pool connections are opened lazily using the same procedure; a pool connection failure during a transfer is a transfer failure (see sync.md Errors).

## Directory Listing

Directory listing uses its own connection per peer, outside the transfer pool. During multi-tree traversal, directory listings for all reachable peers at each level must be issued concurrently, not sequentially. With N reachable peers, the wall-clock time for listing one directory level should be approximately the time of the slowest peer, not the sum of all peers.

## Trace Logging

When verbosity level is `trace` (`-vl trace`), log every pool change:

```
endpoint=<user@host> connections=<n>/<max>
```

Logged on every acquire and release. (`endpoint` is the pool key — user+host of the SFTP URL — not the URL itself, since URLs sharing the same user+host share a pool.) These pool events are emitted only at `-vl trace`; they are absent from stdout at `-vl error`, `-vl info`, and `-vl debug`.

## Parallel directory listing test

This is a code examination test, not a runtime test. The test reads the source files that implement the multi-tree traversal and verifies that directory listings across peers are issued concurrently. Specifically, it must confirm that the code does not list peers sequentially (e.g., awaiting each peer's listing before starting the next). Look for patterns such as: all peer listings collected into a concurrent join/gather/parallel construct rather than a sequential loop with individual awaits.

## Pipelined transfer test

This is a code examination test, not a runtime test. The test reads the source files that implement file transfers and verifies that each transfer uses two concurrent tasks connected by a bounded channel: one task reading from the source peer and one task writing to the destination peer. Specifically, it must confirm that reads and writes are not performed sequentially in a single loop (e.g., read a chunk then write it, then read the next). Look for patterns such as: two spawned tasks or futures, a bounded channel connecting them, and concurrent execution (join/gather/select) rather than alternating read-write in one task.
