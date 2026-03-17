# Peers

How KitchenSync knows where to sync.

## Configuration

Each sync root has a `.kitchensync/peers.conf` file containing optional settings and peer definitions. Run `kitchensync --help` for the full list of settings and their defaults.

The queue stores paths awaiting sync with each peer. When the queue overflows, recent changes get priority (oldest entries are dropped). The peer walk catches anything that overflowed -- it's the source of truth, the queue is just an optimization for fast fan-out.

**Why a cap?** A peer offline for a month could accumulate unbounded queue entries. The cap keeps memory/disk usage bounded. With 10,000 entries and reasonable path lengths, each peer's queue stays under a few MB.

**Why re-walk?** If a peer doesn't run KitchenSync, external changes (direct edits, other sync tools) won't be pushed to us. Periodic re-walks catch these changes. The default of 12 hours balances catching external changes against the cost of walking large peers.

## Global Settings

Optional settings appear before peer definitions, one per line:

```
queue-max-size 10000
retry-interval 30

nas
  sftp://bilbo@192.168.1.50/volume1/docs
```

Format: setting name, whitespace, value. Unrecognized settings are errors.

## Peers

Each peer is defined by a name followed by one or more indented URLs:

```
peer-name
  url1
  url2
  ...
```

The peer name must be a valid filesystem basename (used for `.kitchensync/PEER/{name}.db`). Peer names must match `[a-zA-Z0-9][a-zA-Z0-9_-]*` (start with alphanumeric, followed by alphanumeric/underscore/hyphen). Maximum length: 64 characters. It does not need to match any hostname -- pick something descriptive.

URLs are tried in order. KitchenSync uses the first URL that successfully connects. This allows a single peer to be reachable via multiple paths -- e.g., local network when home, public DNS when remote, Tailscale as fallback.

Blank lines and lines starting with `#` are ignored.

### Per-Peer Settings

Per-peer settings appear as indented lines before the peer's URLs. A line is a per-peer setting if it matches `<setting-name> <value>` where `<setting-name>` is a recognized per-peer setting. Currently the only per-peer setting is `rewalk-after-minutes`. Unrecognized per-peer setting names are errors (same as global settings). All per-peer settings must appear before any URLs for that peer.

## URL Schemes

Supported schemes:

- `sftp://user@host/path/to/tree` -- remote peer over SFTP/SSH (port 22)
- `sftp://user@host:port/path/to/tree` -- remote peer on non-standard SSH port
- `sftp://user:password@host/path/to/tree` -- with inline password (see Authentication)
- `file:///path/to/tree` -- local filesystem path (mounted drive, USB, etc.)

If the password contains special characters (`@`, `:`, `/`, `%`), they must be percent-encoded (e.g., `@` becomes `%40`, `:` becomes `%3A`). Standard URL encoding rules apply to the password component.

SFTP paths are absolute from the filesystem root, not relative to the user's home directory. Use `/home/user/path` rather than `~/path` or `path`.

On Windows, `file://` URLs use forward slashes and include the drive letter: `file:///C:/Users/bilbo/docs`.

## Peer Filesystem Abstraction

Both `file://` and `sftp://` URLs use the same internal interface for filesystem operations (stat, read, write, list directory, rename, delete). The sync logic is identical regardless of transport -- only the underlying I/O differs. All write operations create parent directories as needed.

This abstraction exists so that:
1. All sync logic can be tested using `file://` URLs (no SFTP server required)
2. The SFTP implementation is a thin layer over the SSH library
3. Future transports (e.g., `s3://`, `webdav://`) could be added without changing sync logic

## Example

A complete `peers.conf` showing all settings at their default values:

```
# Global settings (values shown are defaults)
queue-max-size 10000
connection-timeout 30
retry-interval 60
workers-per-peer 10
xfer-cleanup-days 2
back-retention-days 90
tombstone-retention-days 180
log-retention-days 32

# Home NAS -- try local IP first, then Tailscale
nas
  rewalk-after-minutes 720
  sftp://bilbo@192.168.1.50/volume1/docs
  sftp://bilbo@nas.tail12345.ts.net/volume1/docs

# Work laptop -- Tailscale only, re-walk more frequently
work-laptop
  rewalk-after-minutes 120
  sftp://bilbo@work.tail12345.ts.net/home/bilbo/docs

# USB backup drive -- local filesystem
usb-backup
  rewalk-after-minutes 720
  file:///media/bilbo/usb-backup/docs

# Cloud server -- public DNS
cloud
  rewalk-after-minutes 720
  sftp://bilbo@myserver.example.com/home/bilbo/docs
  sftp://bilbo@myserver.example.com:2222/home/bilbo/docs
```

Per-peer settings (like `rewalk-after-minutes`) appear as indented lines before the URLs. Global settings appear at the top, before any peer definitions.

## Connection Order

For each peer, URLs are tried top-to-bottom. Each SSH connection attempt times out after `connection-timeout` seconds (applies to connection establishment only, not to individual file operations).

1. Try first URL. If connection succeeds, use it.
2. If first URL fails (timeout, refused, auth failure), try second URL.
3. Continue until one succeeds or all fail.
4. If all fail and queue is non-empty, wait `retry-interval` seconds, then retry from step 1.

Why sequential, not parallel? Parallel connection attempts would waste resources when the preferred URL (usually fastest, like local network) is available. The user controls priority by URL order.

Each peer has a connection manager thread. It connects when there's work to do: either the queue is non-empty, or it's time for a periodic re-walk. The retry loop only applies when the queue is non-empty but all URLs fail. If the connection drops mid-transfer, workers log an error and the connection manager retries.

The `retry-interval` timer starts after the connection attempt concludes (either success, timeout, or refusal). If a connection times out after 30 seconds and `retry-interval` is 60, the next attempt begins 60 seconds after the timeout -- 90 seconds total from the first attempt.

## Authentication

KitchenSync authenticates SFTP connections using a fallback chain:

1. **Inline password** -- If the URL contains a password (`sftp://user:password@host/path`), use it.
2. **SSH agent** -- If `SSH_AUTH_SOCK` is set and the agent is reachable, request authentication through it.
3. **`~/.ssh/id_ed25519`** -- If the file exists, attempt public-key auth.
4. **`~/.ssh/id_ecdsa`** -- If the file exists, attempt public-key auth.
5. **`~/.ssh/id_rsa`** -- If the file exists, attempt public-key auth.

KitchenSync stops at the first method that succeeds and never prompts interactively.

**Strongly prefer SSH keys over inline passwords.** Inline passwords are stored in plaintext in `peers.conf`. Use them only when key-based auth is impossible (e.g., appliances that don't support authorized_keys). For key-based setup: `ssh-copy-id user@host`

Host key verification uses `~/.ssh/known_hosts`. Unknown hosts are rejected with an error logged and printed to stdout (causing exit per configuration error rules). Use `ssh-keyscan host >> ~/.ssh/known_hosts` to add hosts.

## Unreachable Peers

If all URLs for a peer fail, the connection manager logs the failure and waits `retry-interval` seconds before retrying. This continues indefinitely in watch mode. Changes continue to accumulate in the peer's SQLite queue, ready for fast sync when the peer becomes reachable.

In `--once` mode, if a peer is unreachable after trying all URLs, it is skipped. KitchenSync logs a warning and continues with reachable peers.

## Peer Databases

Each configured peer has a database at `.kitchensync/PEER/{peer-name}.db` containing the snapshot, queue, and config tables. Queues and snapshots persist across runs, enabling fast sync and accurate deletion detection when a peer reconnects. At startup, peer databases for peers not listed in `peers.conf` are deleted -- this cleans up after peer removal. When a connection is established, the snapshot table is updated incrementally by walking the peer's filesystem. See `database.md` for schema details.
