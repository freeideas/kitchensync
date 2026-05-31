# peer Architecture

## Scope

The `peer` module owns startup peer reachability and per-run peer session
identity. It turns parsed `PeerSpec` values from `cli` into connected
`PeerSession` values with stable per-run ids, normalized peer identities,
selected winning URLs, declared roles, effective roles, connected transport
handles, and startup snapshot-existence state supplied after snapshot loading.

This module is responsible for:

- preserving peer operand order and declared peer roles;
- normalizing peer URLs before identity comparison, logging, session
  construction, or snapshot association;
- preserving per-URL connection settings separately from normalized identity;
- probing each logical peer's fallback URLs in command-line order while
  allowing different logical peers to connect concurrently;
- selecting the first candidate URL whose transport can connect and whose root
  is usable under the current run mode;
- constructing local or SFTP transport handles through `TransportFactory` with
  the root mode selected from the run mode;
- applying SFTP authentication order and known-host verification during startup
  connection establishment;
- requesting create-missing peer-root mode in normal mode and require-existing
  peer-root mode in dry-run mode;
- reporting unreachable logical peers and returning structured startup
  failures for peer reachability and role-resolution errors;
- resolving effective peer roles after startup snapshot loading reports whether
  each reachable peer already had `.kitchensync/snapshot.db`.

The module does not parse command-line syntax, validate unsupported URL query
parameter names, implement transport filesystem operations, own snapshot
database lifecycle, make per-path sync decisions, execute file replacement, or
render diagnostics and progress.

## Internal Design

`peer` should be implemented as a startup coordinator with private helpers for
normalization, candidate connection, and role resolution. The coordinator keeps
one logical peer per input `PeerSpec`; fallback URLs are alternatives for that
same logical peer, not separate peers.

Core internal concepts:

- `PeerCandidate`: one parsed and normalized candidate URL plus per-URL
  connection settings such as connection timeout and SFTP idle keep-alive.
- `NormalizedPeerIdentity`: the identity form derived from a candidate URL
  after applying the required normalization rules, excluding query parameters
  and other connection-only settings.
- `PendingPeerSession`: a reachable peer after fallback selection but before
  snapshot-existence role resolution. It contains `PeerId`, declared role,
  selected candidate, normalized identity, transport handle, and invocation
  position.
- `PeerSession`: the exported connected peer handle after role resolution. It
  contains all pending-session fields plus effective role and whether startup
  snapshot data existed.

Fallback attempt state, authentication details, host-key lookup results, and
URL parser details should remain private. Later modules should receive only the
selected transport handle and peer metadata they need for traversal,
operations, snapshots, diagnostics, and progress.

## Startup Flow

The main connection flow is:

1. Receive validated `PeerSpec` values, `RunConfig`, `TransportFactory`, and
   `DiagnosticSink` from the root startup path.
2. Assign stable per-run `PeerId` values in invocation order.
3. Normalize every candidate URL for each logical peer:
   - lowercase scheme and SFTP hostname;
   - remove default SFTP port `22`;
   - collapse consecutive slashes in the path;
   - remove trailing path slash unless that would make the path empty;
   - convert bare paths to `file://` URLs;
   - resolve `file://` paths to absolute paths from the invocation working
     directory;
   - percent-decode unreserved characters;
   - strip query parameters from normalized identity;
   - insert the current OS user into SFTP URLs that omit a username.
4. Start connection establishment for logical peers concurrently when the root
   runtime permits it. Within one logical peer, try fallback candidates
   sequentially in command-line order.
5. For each `file://` candidate, create a local transport handle. Connection
   timeout and idle keep-alive settings do not apply.
6. For each `sftp://` candidate, connect with that candidate's effective
   connection timeout and idle keep-alive setting. Authentication attempts use
   inline password, SSH agent, `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, then
   `~/.ssh/id_rsa`. Host keys are verified through `~/.ssh/known_hosts`;
   unknown or mismatched keys fail the candidate.
7. Pass the candidate transport root mode selected from the run mode. In normal
   mode, request creation of missing roots and parents before accepting the
   candidate. In dry-run mode, require the root to already exist.
8. Select the first successful candidate as the peer's winning URL. Do not try
   remaining fallbacks again later in the same run.
9. If all candidates for one logical peer fail, mark only that peer unreachable
   and emit one error-level diagnostic for the run.
10. After all peer operands have been attempted, fail startup if fewer than two
    peers are reachable or if the declared canon peer is unreachable.
11. Return `PendingPeerSession` values in invocation order for startup snapshot
    loading.

The role-resolution flow is separate:

1. Receive the post-snapshot-loading pending session set and
   snapshot-existence results for those peers from startup snapshot loading.
   This set is authoritative; callers must remove any peer whose snapshot
   recovery or download failed before role resolution.
2. Before assigning effective roles, reapply the startup reachability checks to
   that authoritative set: fail if fewer than two peers remain reachable or if
   the declared canon peer is no longer present.
3. Treat snapshot existence as true only when `.kitchensync/snapshot.db` existed
   on that peer after normal-mode snapshot SWAP recovery and before local empty
   snapshot creation.
4. Resolve effective roles:
   - a declared canon peer is contributing and authoritative even without a
     snapshot;
   - a declared subordinate peer is subordinate;
   - a reachable non-canon peer without snapshot history is automatically
     subordinate for this run;
   - a reachable normal peer with snapshot history is contributing.
5. If no reachable peer has snapshot data and no canon peer is declared, return
   the exact startup failure `First sync? Mark the authoritative peer with a
   leading +`.
6. If no contributing peer remains, return the exact startup failure
   `No contributing peer reachable - cannot make sync decisions`.
7. Return final `PeerSession` values in invocation order, including subordinate
   peers.

## Dependencies

`peer` consumes root-owned contracts:

- `RunConfig` for dry-run mode, connection timeout defaults, and SFTP idle
  keep-alive defaults;
- `PeerSpec`, `PeerRole`, `PeerUrl`, and `PeerId` for parsed operands and stable
  peer identity handles;
- `TransportFactory`, `TransportRootMode`, and `TransportHandle` for local and
  SFTP connection construction;
- `TransportError` categories needed to interpret startup connection and root
  checks;
- `DiagnosticSink` for skipped or unreachable peer diagnostics.

`peer` may use language/runtime URL, path, SSH, SFTP, and known-host libraries
internally. Those library types must not cross the module boundary.

`peer` must not depend on sibling implementation modules:

- `cli` supplies parsed `PeerSpec` values and owns CLI syntax errors;
- `transport` implements file and SFTP filesystem operations behind
  `TransportHandle`;
- `snapshot` performs snapshot recovery, download, creation, inspection,
  mutation, and upload, then supplies snapshot-existence results back to peer
  role resolution;
- `sync` owns traversal and per-path decisions;
- `operations` owns safe peer-side mutations after startup;
- `runtime` owns rendering, verbosity, progress, copy scheduling, and exit-code
  mapping.

## Exported Surface

The public API should stay equivalent to:

```text
connect_peers(run_config, peer_specs, transport_factory, diagnostics)
  -> startup failure
  -> pending sessions requiring snapshot existence resolution

resolve_roles(pending_sessions, snapshot_existence_by_peer)
  -> startup failure
  -> reachable PeerSession list
```

`PeerSession` exposes only:

- `PeerId`, unique within the run;
- normalized peer identity URL;
- selected winning URL with connection settings applied;
- declared role from the peer operand;
- effective role: canon, contributing normal, or subordinate;
- connected `TransportHandle`;
- whether the peer had an existing snapshot at startup.

No public peer API should expose fallback iteration state, authentication
library objects, host-key storage internals, URL parser details, or
transport-specific SFTP handles.

## Child Modules

`peer` should remain a leaf module for now. Its responsibilities are cohesive
startup behavior that all feeds directly into connected peer session
construction and effective role resolution.

If implementation size later needs internal structure, use private files or
helpers such as `normalize`, `connect`, and `roles`. Do not introduce
tree-visible child modules or child architecture/API surfaces unless another
module genuinely needs a narrower shared contract.
