# peer API

Rust module path: `kitchensync::peer`.

The `peer` module exposes the startup contract that turns parsed peer operands
into connected, role-resolved peer sessions. It does not expose URL parser
internals, fallback attempt state, authentication library objects, known-host
storage details, or transport-specific SFTP handles.

## Public Types

The API uses these root-owned shared contracts:

- `RunConfig`: dry-run mode, connection timeout default, and SFTP idle
  keep-alive default.
- `PeerSpec`: one logical peer operand with declared role and ordered fallback
  URL candidates.
- `PeerRole`: declared role from the command line: canon, subordinate, or
  normal.
- `PeerId`: stable per-run peer identifier assigned in invocation order.
- `PeerUrl`: parsed peer URL plus per-URL connection settings.
- `TransportFactory`: factory used to construct local and SFTP transports.
- `TransportHandle`: connected peer filesystem handle.
- `TransportRootMode`: root usability policy selected from dry-run mode.
- `DiagnosticSink`: sink for unreachable-peer diagnostics.

### `PendingPeerSession`

```rust
pub struct PendingPeerSession {
    pub id: PeerId,
    pub invocation_index: usize,
    pub normalized_identity: PeerUrl,
    pub selected_url: PeerUrl,
    pub declared_role: PeerRole,
    pub transport: TransportHandle,
}
```

`PendingPeerSession` represents a reachable peer after fallback URL selection
and root usability checks, but before startup snapshot existence has been
reported. Values are returned in original peer operand order after unreachable
peers have been removed.

`normalized_identity` is the identity form used for peer comparison, logging,
session construction, and snapshot association. It has query parameters and
other connection-only settings removed. `selected_url` is the winning URL for
this run and retains the effective per-URL connection settings that were used
to connect.

### `EffectivePeerRole`

```rust
pub enum EffectivePeerRole {
    Canon,
    Contributing,
    Subordinate,
}
```

`EffectivePeerRole` is the role later modules use for sync decisions. A canon
peer is authoritative and contributing. A contributing peer participates in
decision making without being canon. A subordinate peer receives group outcomes
but does not contribute decisions for this run.

### `PeerSession`

```rust
pub struct PeerSession {
    pub id: PeerId,
    pub invocation_index: usize,
    pub normalized_identity: PeerUrl,
    pub selected_url: PeerUrl,
    pub declared_role: PeerRole,
    pub effective_role: EffectivePeerRole,
    pub transport: TransportHandle,
    pub had_startup_snapshot: bool,
}
```

`PeerSession` is the final connected peer handle exposed to snapshot, sync,
operations, and runtime code. It is stable for one run. The contained
`TransportHandle` is the only transport other modules may use for that peer;
fallback URLs are not exposed and are not retried after `selected_url` wins.

### `SnapshotExistence`

```rust
pub struct SnapshotExistence {
    pub peer_id: PeerId,
    pub existed: bool,
}
```

`SnapshotExistence` is supplied by startup snapshot loading. `existed` is true
only when `.kitchensync/snapshot.db` existed on that peer after normal-mode
snapshot SWAP recovery and before local empty snapshot creation.

### `PeerStartupError`

```rust
pub enum PeerStartupError {
    TooFewReachablePeers,
    DeclaredCanonUnreachable { peer_id: PeerId },
    FirstSyncNeedsCanon,
    NoContributingPeerReachable,
}
```

`FirstSyncNeedsCanon` renders as the exact message:

```text
First sync? Mark the authoritative peer with a leading +
```

`NoContributingPeerReachable` renders as the exact message:

```text
No contributing peer reachable - cannot make sync decisions
```

Per-candidate failures are not part of this public error type. The module
continues through fallback URLs internally, emits one error-level diagnostic for
each logical peer whose candidates all fail, and reports only terminal startup
failures through `PeerStartupError`.

## Public Functions

### `connect_peers`

```rust
pub async fn connect_peers(
    run_config: &RunConfig,
    peer_specs: &[PeerSpec],
    transport_factory: &dyn TransportFactory,
    diagnostics: &dyn DiagnosticSink,
) -> Result<Vec<PendingPeerSession>, PeerStartupError>;
```

`connect_peers` assigns stable per-run `PeerId` values in invocation order,
normalizes every candidate URL, probes logical peers concurrently when the
runtime permits it, tries fallback URLs for each logical peer sequentially in
command-line order, selects the first usable URL for each reachable peer, and
returns pending sessions in invocation order.

For `file://` candidates, the module creates a local transport handle. For
`sftp://` candidates, it establishes SSH/SFTP using the candidate's effective
connection timeout and idle keep-alive settings, authenticates in the required
order, and verifies host keys through `~/.ssh/known_hosts`.

Normal mode passes `TransportRootMode::CreateMissing` so transport construction
creates missing peer roots and parents before accepting a candidate. Dry-run
mode passes `TransportRootMode::RequireExisting` so a candidate whose root does
not already exist is rejected without mutation.
If all candidates fail for one logical peer, only that peer is skipped and one
error diagnostic is emitted. Startup fails if fewer than two peers remain
reachable or if the declared canon peer is unreachable.

### `resolve_roles`

```rust
pub fn resolve_roles(
    pending_sessions: Vec<PendingPeerSession>,
    snapshot_existence: &[SnapshotExistence],
) -> Result<Vec<PeerSession>, PeerStartupError>;
```

`resolve_roles` consumes the pending sessions and applies effective roles after
startup snapshot loading reports snapshot existence.

Role resolution rules are:

- a declared canon peer becomes `EffectivePeerRole::Canon`, even without a
  startup snapshot;
- a declared subordinate peer becomes `EffectivePeerRole::Subordinate`;
- a reachable non-canon peer without startup snapshot history becomes
  `EffectivePeerRole::Subordinate`;
- a reachable normal peer with startup snapshot history becomes
  `EffectivePeerRole::Contributing`.

If no reachable peer had snapshot data and no canon peer was declared,
`resolve_roles` returns `PeerStartupError::FirstSyncNeedsCanon`. If no
contributing peer remains after role resolution, it returns
`PeerStartupError::NoContributingPeerReachable`.

The returned `PeerSession` values preserve invocation order and include
subordinate peers.

## Ownership Rules

`connect_peers` borrows run configuration, peer operands, transport factory,
and diagnostic sink for the duration of startup connection establishment. It
owns each successful `TransportHandle` by moving it into a `PendingPeerSession`.

`resolve_roles` takes ownership of `PendingPeerSession` values and moves their
transport handles into final `PeerSession` values. Callers should not clone or
share transport handles unless the root transport contract explicitly permits
that handle type to be cloned.

`PeerId`, `normalized_identity`, `selected_url`, declared role, effective role,
and `had_startup_snapshot` are immutable session metadata for the rest of the
run. Later modules may rely on them for diagnostics, snapshot association,
traversal, operations, and progress, but they must not expect fallback
reselection after startup.
