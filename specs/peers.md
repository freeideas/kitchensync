# Peers

How KitchenSync knows where to sync.

## Configuration

Each sync root has a `.kitchensync/peers.conf` file listing one peer per line. Each peer is a URL specifying the full path to the corresponding directory on that peer.

Supported URL schemes:

- `sftp://user@host/path/to/tree` — remote peer accessed over SFTP/SSH (port 22)
- `sftp://user@host:port/path/to/tree` — remote peer on a non-standard SSH port
- `file:///path/to/tree` — local filesystem path (e.g. mounted drive, another directory)

Blank lines and lines starting with `#` are ignored.

## Example

```
# NAS over SSH
sftp://ace@nas.local/volume1/docs

# USB drive
file:///media/ace/usb-backup/docs

# Another machine
sftp://ace@workstation/home/ace/docs
```

## Authentication

SFTP peers require passwordless SSH access. KitchenSync authenticates using a strict fallback chain, tried one at a time in this order:

1. **SSH agent** — If `SSH_AUTH_SOCK` is set and the agent is reachable, request authentication through it. If the agent is reachable but has no accepted key, this step fails and falls through.
2. **`~/.ssh/id_ed25519`** — If the file exists, attempt public-key auth with it.
3. **`~/.ssh/id_ecdsa`** — If the file exists, attempt public-key auth with it.
4. **`~/.ssh/id_rsa`** — If the file exists, attempt public-key auth with it.

KitchenSync stops at the first method that succeeds. If all methods fail (or none are available), the connection fails and the peer is treated as unreachable (see "Unreachable Peers" below).

No auth configuration is needed in `peers.conf` — if `ssh user@host` works without a password, KitchenSync will too.

Setup: `ssh-copy-id user@host`

## Unreachable Peers

Peers that are unreachable (network down, drive not mounted, SSH connection refused) are skipped. KitchenSync logs an `error`-level entry to the database (`.kitchensync/kitchensync.sqlite`, see `quartz-lifecycle.md`) and continues with the remaining peers.
