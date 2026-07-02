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
interface. Startup keeps the selected peer connection state as the reachable
peer handle, including the established SSH/SFTP session and remote root path for
an `sftp://` peer, but this must not change the externally visible rule:
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
4. First successful connection wins. The reachable peer handle records that
   connection and root. Remaining URLs are not tried.
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

## Progress Output

At `info`, `debug`, and `trace` verbosity, KitchenSync emits one plain line per
action to stdout during sync execution, in the order the actions happen. At
`error` verbosity, these progress lines are suppressed. There is no live status
screen, progress bar, percentage, scanned-directory indicator, or terminal
control sequence. Output is identical whether or not stdout is a terminal.

Each line is an action letter, a single space, then the slash-separated relative
path from the sync root:

```text
C path/to/file.ext
X path/to/file.ext
```

- `C <relpath>` - the file is being copied from one peer to one or more other
  peers. One line per path, regardless of how many peers receive it.
- `X <relpath>` - the path is being deleted (displaced to BAK/) on one or more
  peers. One line per path. Files and directories use the same letter.

No line is emitted for directory creation, listing, snapshot work, or BAK/TMP
cleanup. These lines are `info`-level. Errors and the final `sync complete`
message are separate output and remain visible.

## Trace Logging

When verbosity level is `trace`, include copy-slot acquire and release events
in the plain logs:

```text
copy-slots active=<n>/<max>
```

These events describe global active copy slots, not network connections.
