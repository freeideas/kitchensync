# peer:

## Purpose

Own peer identity, URL normalization, fallback URL selection, startup connection
establishment, peer root handling, SFTP authentication and host-key
verification, and effective peer role resolution for a run.

The module turns parsed peer operands into reachable peer sessions. Each session
has a stable per-run peer id, normalized peer identity, selected winning URL,
declared role, effective role, connected transport handle, and snapshot
existence state supplied by startup snapshot loading. It decides which URL wins
for each peer during startup and which reachable peers are contributing, canon,
or subordinate after snapshot existence is known.

## Responsibilities

- Accept root `PeerSpec` values produced by the CLI, preserving operand order,
  declared role (`canon`, `subordinate`, or `normal`), and ordered fallback URL
  candidates for each logical peer.
- Normalize every peer URL before identity comparison, logging, session
  construction, or snapshot association:
  - lowercase scheme and SFTP hostname;
  - remove default SFTP port `22`;
  - collapse consecutive slashes in the path;
  - remove a trailing path slash except where doing so would make the path
    empty;
  - convert bare paths to `file://` URLs;
  - resolve `file://` paths to absolute paths from the invocation working
    directory;
  - percent-decode unreserved characters;
  - strip query parameters from the normalized identity;
  - insert the current OS user into SFTP URLs that omit a username.
- Preserve per-URL connection settings separately from normalized identity.
  `timeout-conn` overrides the run connection timeout for that URL only, and
  `timeout-idle` overrides the run SFTP idle keep-alive setting for that URL
  only.
- Start connection establishment for all peer operands concurrently. A peer
  with fallback URLs still tries its own URLs sequentially: primary first, then
  each fallback in command-line order.
- Select the first URL for a peer that can be connected and whose sync root can
  be made usable under the current run mode. Once selected, that URL remains the
  peer's only URL for all later operations in the same run; remaining fallbacks
  are not tried again.
- For `file://` candidates, create a local transport handle. Connection timeout
  and idle keep-alive settings do not apply.
- For `sftp://` candidates, establish SSH/SFTP using the candidate's effective
  connection timeout and idle keep-alive settings. Authentication attempts must
  use this order: inline password, SSH agent, `~/.ssh/id_ed25519`,
  `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`.
- Verify SFTP host keys with `~/.ssh/known_hosts`; an unknown or mismatched host
  key makes that URL fail.
- In normal mode, if a candidate's peer root path does not exist, request
  create-missing root mode during transport construction so the root and any
  missing parents exist before accepting it as connected.
- In dry-run mode, request require-existing root mode during transport
  construction. Do not create peer roots or parents. A candidate whose root path
  does not already exist fails for that run.
- Treat failure to create a missing peer root in normal mode as failure of that
  URL, then continue to the next fallback if one exists.
- Report each logical peer for which all URL candidates fail as unreachable for
  the run, with an error-level diagnostic sent through `DiagnosticSink`.
- After initial connection establishment, require at least two reachable peers.
  If fewer than two remain reachable, return a startup failure to the root.
- If the declared canon peer is unreachable, return a startup failure to the
  root.
- Accept snapshot existence results from the startup snapshot-loading step. A
  snapshot exists only when `.kitchensync/snapshot.db` existed on that peer after
  any normal-mode snapshot SWAP recovery and before local empty snapshot
  creation.
- Treat the peer set supplied after startup snapshot loading as authoritative
  for role resolution. If snapshot recovery or download caused peers to be
  excluded, reapply the fewer-than-two-reachable and canon-unreachable startup
  checks to the remaining pending sessions before resolving effective roles.
- Apply effective roles after snapshot existence is known:
  - a declared canon peer is contributing and authoritative even when it had no
    snapshot;
  - a declared subordinate peer is subordinate for this run;
  - a reachable non-canon peer whose snapshot did not exist is automatically
    subordinate for this run;
  - a reachable normal peer with existing snapshot history is contributing.
- If no reachable peer has snapshot data and no canon peer is declared, return
  the first-sync startup failure with the exact message `First sync? Mark the
  authoritative peer with a leading +`.
- If no contributing peer remains after declared and automatic subordinate
  roles are applied, return the startup failure with the exact message
  `No contributing peer reachable - cannot make sync decisions`.
- Expose the final reachable `PeerSession` set in invocation order, including
  subordinate peers, so later sync traversal can list them and apply group
  outcomes to them.

## Boundaries

The peer module owns startup reachability and per-run peer session identity. It
does not own command-line option parsing, help text, minimum operand validation,
or validation of unsupported URL query parameter names; those belong to `cli`.

The peer module constructs connected transport handles through
`TransportFactory` and chooses the root creation policy for each candidate, but
it does not implement `file://` or `sftp://` filesystem operations, normalize
operation error categories, filter non-regular entries, or decide listing retry
behavior after startup; those belong to `transport` and `sync`.

The peer module does not recover, download, create, inspect, mutate, or upload
SQLite snapshot databases. Snapshot lifecycle code supplies snapshot existence
results back to peer startup so this module can resolve effective roles.

The peer module does not make per-path sync decisions. It only marks whether a
reachable peer contributes to decisions for this run, is the canon authority, or
is subordinate. File, directory, type-conflict, and deletion decisions belong to
`sync`.

The peer module does not perform file replacement, displacement to BAK, SWAP
recovery for user entries, TMP/BAK cleanup, or dry-run suppression after
startup. Those operations belong to `operations`.

The peer module emits diagnostic events for skipped unreachable peers and
returns structured startup failures, but it does not render stdout, manage
verbosity filtering, show progress, or map terminal results to process exit
codes. Those responsibilities belong to `runtime` and root startup glue.

## Error Obligations

- A URL candidate failure is local to that candidate. Continue trying fallback
  candidates for the same peer until one succeeds or all fail.
- SFTP handshake timeout, authentication failure, host-key rejection, missing
  root in dry-run mode, and failed root creation in normal mode all make the
  current URL candidate fail.
- If all candidates for one logical peer fail, mark only that peer unreachable
  and emit one error-level diagnostic identifying the peer or attempted URL set.
- Startup must fail after reachability if fewer than two peers are reachable.
- Startup must fail after reachability if the declared canon peer is
  unreachable.
- Startup must recheck the fewer-than-two-reachable and declared-canon
  conditions after snapshot loading excludes any peer because snapshot recovery
  or download failed.
- Startup must fail after snapshot existence role resolution when no snapshot
  history exists anywhere and no canon peer is declared.
- Startup must fail after role resolution when every remaining reachable peer is
  subordinate and therefore no peer can contribute decisions. A canon peer
  counts as contributing for this check.
- Later transport failures after a winning URL has been selected are not handled
  as fallback reselection by this module.

## Exposed Contract

The module exposes an operation equivalent to:

```text
connect_peers(run_config, peer_specs, transport_factory, diagnostics)
  -> startup failure
  -> pending sessions requiring snapshot existence resolution

resolve_roles(pending_sessions, snapshot_existence_by_peer)
  -> startup failure
  -> reachable PeerSession list
```

`pending_sessions` passed to `resolve_roles` is the post-snapshot-loading
reachable set. Callers must remove peers whose snapshot recovery or download
failed before calling it; `resolve_roles` still enforces the startup reachability
and canon checks on that reduced set.

`PeerSession` is stable for the run and contains:

- `PeerId`, unique within the run;
- normalized peer identity URL;
- selected winning URL with connection settings applied;
- declared role from the peer operand;
- effective role for decisions: canon, contributing normal, or subordinate;
- connected `TransportHandle`;
- whether the peer had an existing snapshot at startup.
