# 002_peer-startup-and-identity: Peer startup and identity

## Behavior
This concern derives from `specs/sync.md` sections "Peers", "Fallback URLs",
"Per-URL Settings", "Canon Peer (`+`)", "Subordinate Peer (`-`)", "Startup",
and "Errors", and `specs/concurrency.md` sections "Fallback URLs" and
"Connection Establishment". It covers how accepted peer arguments are grouped
into peers, how fallback URLs are tried, how roots are created or rejected, how
reachable and unreachable peers affect startup, how first sync and canon rules
are enforced, and how peers without snapshot history become subordinate.

## $REQ_IDs
- `002.1` -- Each accepted peer argument without square brackets identifies one peer with one candidate URL.
- `002.2` -- Each accepted peer argument with square brackets identifies one peer whose candidate URLs are the bracket contents in their written order.
- `002.3` -- A leading `+` on an accepted peer argument makes that whole peer the canon peer.
- `002.4` -- A leading `-` on an accepted peer argument makes that whole peer subordinate.
- `002.5` -- Startup attempts peer connections for all peers in parallel.
- `002.6` -- Startup tries a peer's primary URL before any fallback URLs.
- `002.7` -- Startup tries a peer's fallback URLs in their written order.
- `002.8` -- Startup selects the first candidate URL that connects as the peer's winning URL.
- `002.9` -- Startup does not try later candidate URLs for a peer after selecting a winning URL.
- `002.10` -- Every operation for a reachable peer during the run uses that peer's winning URL.
- `002.11` -- Later directory-listing failures do not cause startup fallback URL selection to run again during the same run.
- `002.12` -- Later transfer failures do not cause startup fallback URL selection to run again during the same run.
- `002.13` -- If every candidate URL for a peer fails during startup, that peer is unreachable for the run.
- `002.14` -- A URL's `timeout-conn` query parameter overrides the global connection timeout for that URL's SFTP handshake.
- `002.15` -- A URL's `timeout-idle` query parameter overrides the global idle keep-alive TTL for that URL's SFTP connection.
- `002.16` -- Connection timeout settings do not affect `file://` peer connection establishment.
- `002.17` -- Idle keep-alive settings do not affect `file://` peer connection establishment.
- `002.18` -- In a normal run, startup creates a missing local `file://` peer root and any missing parents before connecting to that URL.
- `002.19` -- In a normal run, startup creates a missing remote `sftp://` peer root and any missing parents before selecting that URL as reachable.
- `002.20` -- In `--dry-run`, startup treats a `file://` URL whose root path does not already exist as a failed URL for that run.
- `002.21` -- In `--dry-run`, startup treats an `sftp://` URL whose root path does not already exist as a failed URL for that run.
- `002.22` -- In `--dry-run`, startup does not create missing peer roots or missing peer root parents.
- `002.23` -- If peer root creation fails in a normal run, startup treats that candidate URL as failed.
- `002.24` -- Startup skips an unreachable non-canon peer and continues the run with the remaining reachable peers.
- `002.25` -- Startup fails when fewer than two peers are reachable.
- `002.26` -- Startup fails when the canon peer is unreachable.
- `002.27` -- If snapshot SWAP recovery or snapshot download fails with an error other than not found, startup excludes that peer from the reachable set.
- `002.28` -- After excluding a peer because snapshot SWAP recovery or snapshot download failed, startup rechecks whether at least two peers remain reachable.
- `002.29` -- After excluding a peer because snapshot SWAP recovery or snapshot download failed, startup rechecks whether the canon peer remains reachable.
- `002.30` -- A reachable non-canon peer whose `.kitchensync/snapshot.db` did not exist on disk at startup is automatically treated as subordinate.
- `002.31` -- A reachable canon peer whose `.kitchensync/snapshot.db` did not exist on disk at startup is not automatically treated as subordinate.
- `002.32` -- A reachable peer with an existing `.kitchensync/snapshot.db` counts as having snapshot history even when its `snapshot` table has no rows.
- `002.33` -- Startup fails when no reachable peer had `.kitchensync/snapshot.db` on disk at startup and no canon peer was designated.
- `002.34` -- The no-snapshots-and-no-canon startup failure prints `First sync? Mark the authoritative peer with a leading +`.
- `002.35` -- Startup fails when all reachable peers are subordinate after auto-subordination.
- `002.36` -- The no-contributing-peer startup failure prints `No contributing peer reachable - cannot make sync decisions`.

## Notes
This category owns peer selection before traversal starts. URL normalization
belongs to `005_path-time-and-url-formats`. Scheme-specific filesystem and
SFTP operations belong to `003_peer-transports`. Path outcome rules for canon
and subordinate peers belong to `007_reconciliation-decisions`.
