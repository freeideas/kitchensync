# PeerConnections:

## Purpose

PeerConnections turns the accepted peer arguments for one KitchenSync run into
the reachable peer set used by the rest of the product. It owns peer grouping,
canon and subordinate markers, startup URL fallback selection, snapshot-history
startup checks, and startup failures that happen before reconciliation can make
file decisions.

This child does not parse general command-line flags. It receives peer
arguments that the command-line layer has already accepted, including any
leading `+` or `-` marker and bracketed fallback list. It produces a startup
result containing the reachable peers, each peer's winning URL, each peer's
role for this run, and each peer's local snapshot database prepared for later
sync work.

## Responsibilities

Peer argument grouping:

- A peer argument without square brackets is one peer with one candidate URL.
- A peer argument with square brackets is one peer with the bracket contents as
  candidate URLs, in their written order.
- A leading `+` applies to the whole peer and marks that peer as canon.
- A leading `-` applies to the whole peer and marks that peer as subordinate.
- Candidate URL text is kept in peer order and candidate order. URL parsing,
  validation, and normalization rules come from FormatRules.

Connection selection:

- Startup begins connection establishment for all peers in parallel.
- Within one peer, the primary URL is tried first, followed by fallback URLs in
  their written order.
- The first candidate URL that connects and satisfies root setup rules becomes
  the peer's winning URL.
- Later candidate URLs for that peer are not tried after a winning URL is
  selected.
- If every candidate URL for a peer fails, the peer is unreachable for the
  run.
- If normal-run peer root creation fails for a candidate URL, that candidate
  URL is treated as failed and the next fallback URL may be tried.
- Every later operation for a reachable peer uses the winning URL handle. Later
  listing failures and transfer failures never restart startup fallback
  selection and never switch the peer to another URL during the same run.

Reachable set rules:

- An unreachable non-canon peer is skipped and the run continues with the
  remaining reachable peers.
- Startup fails if fewer than two peers are reachable.
- Startup fails if the canon peer is unreachable.
- Every unreachable peer, including peers excluded later during snapshot
  startup, emits an error-level diagnostic through the product output surface.

Snapshot startup rules:

- For each reachable peer, PeerConnections asks SnapshotDatabase to perform the
  startup snapshot work required for the current run mode.
- In a normal run, snapshot SWAP recovery is attempted before downloading that
  peer's `.kitchensync/snapshot.db`.
- In a dry run, peer-side snapshot SWAP recovery is skipped and the live
  snapshot is downloaded as-is when present.
- If `.kitchensync/snapshot.db` is not found, the peer remains reachable and a
  new empty local snapshot database is prepared for that peer.
- If snapshot SWAP recovery or snapshot download fails with any error other
  than not found, that peer is excluded from the reachable set, an error-level
  diagnostic is emitted, and the reachable-set checks are repeated.
- After any snapshot-startup exclusion, startup again fails if fewer than two
  peers remain reachable or if the canon peer is no longer reachable.

Snapshot history and role rules:

- A peer had snapshot history for startup if `.kitchensync/snapshot.db` existed
  on disk at the start of that peer's snapshot download step.
- An existing `.kitchensync/snapshot.db` counts as snapshot history even when
  its `snapshot` table has no rows.
- A reachable non-canon peer without snapshot history is automatically treated
  as subordinate for this run.
- A reachable canon peer without snapshot history is not automatically treated
  as subordinate.
- If no reachable peer had snapshot history and no canon peer was designated,
  startup fails before changing user files and writes exactly this stdout line:
  `First sync? Mark the authoritative peer with a leading +`
- If every reachable peer is subordinate after auto-subordination, startup
  fails before reconciliation and writes exactly this stdout line:
  `No contributing peer reachable - cannot make sync decisions`

The startup result exposes only peers that remain reachable after all startup
checks. Each returned peer records:

- its stable peer index from the accepted peer argument order;
- whether it is canon, subordinate, or normal for this run;
- whether it had snapshot history at startup;
- its winning normalized URL and connected transport handle;
- the local snapshot database prepared for sync decisions and later upload.

## Boundaries

PeerConnections depends on PeerTransportSurface for connection attempts and all
peer-root handles. It must not call LocalTransport or SftpTransport directly.
Scheme-specific connection behavior, SFTP authentication, local filesystem
root creation, dry-run root rejection, operation error categories, and the
transport-neutral handle shape belong behind PeerTransportSurface and its
transport implementations.

PeerConnections depends on SnapshotDatabase for snapshot SWAP recovery,
snapshot download, creation of a new empty local snapshot database when the
remote snapshot is not found, and reporting whether the remote snapshot file
existed at startup. It does not define the SQLite schema, snapshot row updates,
snapshot upload, or snapshot replacement mechanics.

PeerConnections depends on FormatRules for URL parsing and normalization. It
does not own URL identity, path hashing, timestamp formats, relative-path
validation, or command-line option validation outside accepted peer grouping.

PeerConnections does not choose file, directory, deletion, or type-conflict
outcomes. SyncTraversal and its decision collaborators use the returned
reachable peer set and roles to make reconciliation decisions. CopyStaging owns
user-file mutation staging. SnapshotDatabase owns snapshot mutation and upload.

PeerConnections does not own progress output, help text, final completion
output, or the formatting of general diagnostics. It is responsible for raising
the startup failure reasons and unreachable-peer diagnostics required here so
the root coordinator can emit stdout-only output and exit with the required
status.

## Invariants

- Candidate URL order for a peer is never reordered.
- A peer has at most one winning URL in a run.
- A reachable peer's winning URL is immutable after startup selection.
- Fallback selection is a startup-only process.
- Startup never returns fewer than two reachable peers.
- Startup never returns a result with an unreachable canon peer.
- Startup never returns a result where all reachable peers are subordinate.
- Startup never allows reconciliation to begin when no reachable peer has
  snapshot history and no canon peer was designated.
