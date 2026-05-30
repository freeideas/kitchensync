# Concurrency And Progress

## Copy Concurrency

KitchenSync limits total file-copy work, not connections. By default, at most
10 file copies may be active at one time across the whole run.

`--max-copies N` sets the global maximum number of active file copies. A copy counts
against this limit whether it is `file://` to `file://`, `file://` to `sftp://`,
`sftp://` to `file://`, or `sftp://` to `sftp://`.

Directory listing, snapshot download/upload, directory creation, and BAK/TMP/SWAP
cleanup do not count as file copies. They may still run concurrently where the
sync algorithm requires it, but they must not allow more than `--max-copies` active
file-copy operations.

Copying is incremental. KitchenSync does not first scan the whole tree and then
start a copy phase. As soon as traversal finds copy work in an early directory,
that work may occupy available copy slots while later directories are still
being scanned.

There is no per-peer, per-host, or per-connection transfer limit in the user
interface. The implementation may keep SFTP sessions open and reuse them as an
internal optimization, but this must not change the externally visible rule:
`--max-copies` means max active file copies for the whole run.

| Setting                   | Default | Global flag      |
| ------------------------- | ------- | ---------------- |
| Max active file copies    | 10      | `--max-copies`   |
| Failed copy tries         | 3       | `--retries-copy` |
| Directory listing tries   | 3       | `--retries-list` |
| SSH connection timeout    | 30s     | `--timeout-conn` |
| SFTP idle keep-alive TTL  | 30s     | `--timeout-idle` |

`--timeout-conn` and `--timeout-idle` apply to SFTP connection management
only. They do not affect local `file://` peers.

## Fallback URLs

A peer can have multiple URLs grouped in square brackets on the command line.
These are fallback network paths to the same data. URLs are tried in order; the
first that connects wins.

```text
kitchensync [sftp://192.168.1.50/photos,sftp://nas.vpn/photos] /local/photos
```

Per-URL query settings apply only to connection establishment and SFTP
keep-alive behavior:

```text
kitchensync "[sftp://192.168.1.50/photos?timeout-conn=20,sftp://nas.vpn/photos?timeout-conn=60&timeout-idle=10]" /local/photos
```

`max-copies` is not a per-URL setting. If a URL contains `max-copies`,
argument validation must reject it with a clear error.

## Connection Establishment

At startup, each peer selects one winning URL:

1. Try the peer's primary URL first, then each fallback URL in order.
2. For SFTP URLs, `--timeout-conn` or the URL's `timeout-conn` parameter bounds the SSH handshake.
   If it expires, try the next URL. After the handshake succeeds, check whether
   the peer's root path exists on the remote server. In normal runs, if it does
   not exist, create it and any missing parents via SFTP. In `--dry-run`, do not
   create it; treat that URL as failed for this run. If creation fails in a
   normal run, the URL is treated as failed.
3. For `file://` URLs, the connection is a lightweight local handle. Connection
   timeout and keep-alive settings do not apply. In normal runs, if the local
   path does not exist, create it and any missing parents before connecting. In
   `--dry-run`, do not create missing local paths; treat that URL as failed for
   this run.
4. First successful connection wins. Remaining URLs are not tried.
5. If all URLs fail, the peer is unreachable for the run.

After startup, all operations for a reachable peer use that peer's winning URL
for the remainder of the run. Fallback URLs are not retried again during the
same run after a winner is selected. A later directory-listing failure is
retried as described in `multi-tree-sync.md`; if it still fails, it becomes a
listing error for that subtree. A later transfer failure is a transfer failure.

## Directory Listing

During multi-tree traversal, directory listings for all reachable peers at each
directory level must be issued concurrently, not sequentially. The
implementation starts listing operations for every reachable peer at that
directory level before awaiting any listing result.

The currently scanned directory is tracked for the live terminal status screen.

## Copy Queue Tries

Queued file-copy work carries its own try count. The queue implementation is
not specified: it may be in memory, on disk, or a mix. The required behavior is
that each queued copy remembers how many times it has already been tried.

`--retries-copy` is the maximum number of total tries for a queued copy,
including the first try. When a copy try fails, KitchenSync increments that
queued copy's try count. If the try count has not reached `--retries-copy`, the
copy is moved to the back of the queue and other queued work continues. If the
try count has reached `--retries-copy`, the copy is marked failed for this run
and is not requeued.

Try limits are global behavior. They apply the same way to local copies, SFTP
copies, and mixed-scheme copies.

## Live Terminal Status

During sync execution, KitchenSync displays a live text status screen similar
to package installers and updaters on Linux. The display may use horizontal
bars, percentages, counts, and concise labels.

The screen must update at most once per second. Faster internal events are
coalesced into the next refresh.

At any moment, the screen shows one row for each active file copy, up to the
configured `--max-copies` limit. Each active-copy row starts with the basename of the
file being copied, not the full path. After the basename, show a horizontal
progress bar that grows toward completion as bytes are copied. When the file is
fully copied, its bar reaches the end before the row disappears or is replaced
by another active copy.

The bottom line of the screen is always the directory currently being scanned.
For the root directory, display `Scanning: .`. For other directories, display
the full slash-separated relative directory path from the sync root.

The live screen may also show completed and failed copy counts, plus an overall
percentage when a meaningful denominator is known. These summaries must not
displace the active-copy rows or the bottom scanning line.

When stdout is not an interactive terminal, KitchenSync must not emit terminal
control sequences. In that mode, it emits plain line-oriented progress at no
more than once per second, with the same information in a readable form.

Errors and final completion messages must remain visible after the live screen
finishes.

## Trace Logging

When verbosity level is `trace`, include copy-slot acquire and release events
in the plain logs:

```text
copy-slots active=<n>/<max>
```

These events describe global active copy slots, not network connections.
