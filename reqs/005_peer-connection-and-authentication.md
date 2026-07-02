# 005_peer-connection-and-authentication: Startup peer reachability and authentication

## Behavior
This concern derives from `specs/sync.md` sections "Authentication (fallback
chain)" and "Startup", `specs/concurrency.md` sections "Fallback URLs" and
"Connection Establishment", and `plan/sftp-client.md`. It covers parallel peer
connection at startup, fallback URL selection, root directory creation in normal
runs, connection timeout settings, SFTP host-key verification, SFTP
authentication fallback order, unreachable peer handling, and startup exit
conditions that depend on reachable peers.

## $REQ_IDs

- `005.1` -- At startup, KitchenSync starts connection establishment for all peer arguments in parallel.
- `005.2` -- For a peer with fallback URLs, KitchenSync tries the peer's primary URL before its fallback URLs.
- `005.3` -- For a peer with fallback URLs, KitchenSync tries fallback URLs in command-line order.
- `005.4` -- For a peer with fallback URLs, KitchenSync selects the first URL whose connection establishment succeeds as that peer's winning URL.
- `005.5` -- For a peer with a winning URL, KitchenSync does not try that peer's remaining fallback URLs during the run.
- `005.6` -- For a reachable peer, KitchenSync uses the peer's winning URL for all later operations during the run.
- `005.7` -- For an SFTP URL without a `timeout-conn` query parameter, `--timeout-conn` bounds the SSH handshake before KitchenSync tries the next URL.
- `005.8` -- For an SFTP URL with a `timeout-conn` query parameter, that URL parameter rather than `--timeout-conn` bounds the SSH handshake before KitchenSync tries the next URL.
- `005.9` -- Connection timeout and SFTP idle keep-alive settings do not affect `file://` peer connection establishment.
- `005.10` -- In a normal run, KitchenSync creates a missing local peer root directory and any missing parents before connecting to a `file://` URL.
- `005.11` -- In a normal run, KitchenSync creates a missing remote peer root directory and any missing parents via SFTP before accepting an `sftp://` URL as connected.
- `005.12` -- If peer root directory creation fails in a normal run, KitchenSync treats that URL as failed.
- `005.13` -- If all URLs for a peer fail at startup, KitchenSync treats that peer as unreachable for the run.
- `005.14` -- For each peer that is unreachable at startup, KitchenSync emits an error-level diagnostic.
- `005.15` -- During startup, KitchenSync exits with an error when fewer than two peers are reachable.
- `005.16` -- During startup, KitchenSync exits with an error when the canon peer is unreachable.
- `005.17` -- KitchenSync rejects an SFTP connection when the server host key is not trusted by `~/.ssh/known_hosts`.
- `005.18` -- KitchenSync tries SFTP authentication credential sources in this order: inline password from the URL, SSH agent, `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`.
- `005.19` -- When an SFTP authentication credential source is absent or rejected, KitchenSync continues with the next credential source in the fallback chain.

## Notes
Dry-run-specific no-create behavior belongs to `018_dry-run.md`. Transport file
operations after connection belongs to `009_transport-operations.md`.
