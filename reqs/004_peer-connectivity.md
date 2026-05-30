# 004_peer-connectivity: Peer connection establishment

## Behavior
This concern derives from `specs/sync.md` sections "Startup", "Authentication (fallback chain)", and "Peer Transports", `specs/concurrency.md` sections "Fallback URLs" and "Connection Establishment", and `specs/README.md` sections "Why KitchenSync?" and "Fallback Paths". It covers establishing reachable peer handles, connection attempts for all peers, fallback URL selection order, local and SFTP root handling, SFTP authentication and host key behavior, use of the winning URL for the rest of the run, unreachable-peer startup outcomes, and the no-KitchenSync-infrastructure-on-peers access model.

## $REQ_IDs
- `004.1` -- At startup, KitchenSync initiates connection establishment for all peer arguments without waiting for earlier peer connection attempts to finish.
- `004.2` -- For a peer with fallback URLs, KitchenSync tries the peer's primary URL before trying any fallback URL.
- `004.3` -- For a peer with fallback URLs, KitchenSync tries fallback URLs in the order supplied on the command line.
- `004.4` -- For a peer with fallback URLs, the first URL that connects becomes that peer's winning URL.
- `004.5` -- After a peer selects a winning URL, KitchenSync uses that URL for all later operations on that peer during the run.
- `004.6` -- After a peer selects a winning URL, KitchenSync does not try that peer's remaining fallback URLs during the same run.
- `004.7` -- If every URL for a peer fails during startup connection establishment, KitchenSync treats that peer as unreachable for the run.
- `004.8` -- KitchenSync logs an error-level diagnostic for each peer skipped as unreachable during startup connection establishment.
- `004.9` -- If fewer than two peers are reachable after startup peer connection establishment, KitchenSync exits with an error.
- `004.10` -- If the canon peer is unreachable after startup peer connection establishment, KitchenSync exits with an error.
- `004.11` -- In a normal run, KitchenSync creates a missing `file://` peer root directory and any missing parents before connecting to that URL.
- `004.12` -- In `--dry-run`, KitchenSync does not create a missing `file://` peer root directory or missing parent directory.
- `004.13` -- In `--dry-run`, a `file://` URL whose peer root directory does not already exist is treated as failed for that run.
- `004.14` -- In a normal run, KitchenSync creates a missing `sftp://` peer root directory and any missing parents before using that URL as the winning URL.
- `004.15` -- In `--dry-run`, KitchenSync does not create a missing `sftp://` peer root directory or missing parent directory.
- `004.16` -- In `--dry-run`, an `sftp://` URL whose peer root directory does not already exist is treated as failed for that run.
- `004.17` -- If creation of a missing peer root directory fails in a normal run, KitchenSync treats that URL as failed.
- `004.18` -- For `sftp://` URLs, KitchenSync applies `--timeout-conn` to bound the SSH handshake during connection establishment.
- `004.19` -- For `sftp://` URLs, a URL query-string `timeout-conn` value overrides `--timeout-conn` for that URL's SSH handshake during connection establishment.
- `004.20` -- If an SFTP SSH handshake exceeds its applicable `timeout-conn`, KitchenSync treats that URL as failed.
- `004.21` -- For `sftp://` URLs, a URL query-string `timeout-idle` value overrides `--timeout-idle` for that URL's SFTP keep-alive behavior.
- `004.22` -- For `file://` URLs, connection timeout settings do not affect connection establishment.
- `004.23` -- For `file://` URLs, idle keep-alive settings do not affect connection establishment.
- `004.24` -- For SFTP authentication, KitchenSync attempts usable credentials in this order: inline password from the URL, SSH agent, `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`.
- `004.25` -- KitchenSync verifies SFTP host keys using `~/.ssh/known_hosts`.
- `004.26` -- KitchenSync rejects SFTP connections to hosts that are not trusted by `~/.ssh/known_hosts`.
- `004.27` -- Bare path peers use local filesystem operations.
- `004.28` -- `file://` URL peers use local filesystem operations.
- `004.29` -- `sftp://` URL peers use SSH/SFTP operations.
- `004.30` -- KitchenSync can sync peers using only the peer paths and URLs supplied on the command line, without requiring KitchenSync-specific infrastructure on those peers.

## Notes
This category owns startup peer reachability after peer operands have been parsed. Peer address syntax and normalized identity belong to `003_peer-addressing`; the transport operation contract, transport error categories, non-regular entry handling, and filename case preservation belong to `015_transport-operations`; snapshot database download and recovery belongs to `006_snapshot-lifecycle`; directory-level listing concurrency and listing-error subtree behavior belong to `007_traversal-and-excludes`; sync decisions, file-copy swap sequencing, and progress display belong to their specific categories.
