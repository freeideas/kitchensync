# StartupCoordinator:

## Purpose

StartupCoordinator chooses which peer URLs are usable for this run. It receives
already-validated peer definitions from PeerConnections, starts startup work for
all peers in parallel, tries each peer's URLs in the required order, records the
first successful URL as that peer's winner, and returns the reachability result
that PeerConnections uses for the rest of startup.

This child coordinates connection establishment but does not implement the
transport-specific establishment rules. It calls the file URL and SFTP URL
connection children to test one URL at a time, and it treats their success or
failure results as the authority for that URL.

## Responsibilities

StartupCoordinator exposes one startup operation. The operation accepts the peer
arguments in caller-provided form, including:

- the peer identity and role, including which peer is canon;
- the primary URL for each peer;
- that peer's fallback URLs in command-line order;
- the already-parsed connection settings and run mode needed by the URL
  establishment children.

For every peer argument, StartupCoordinator starts that peer's establishment
work without waiting for any other peer to finish. Each peer has its own
sequential URL attempt loop. That loop tries the primary URL first, then tries
fallback URLs in command-line order until one URL succeeds or the list is
exhausted.

For a URL attempt, StartupCoordinator dispatches by URL kind:

- `file://` URLs are passed to the file URL connection child.
- `sftp://` URLs are passed to the SFTP URL connection child.

If a URL establishment child reports success, StartupCoordinator records that
URL and the returned connection handle or connection settings as the peer's
winning URL. It then stops that peer's URL attempt loop and does not try any
later fallback URL during this run.

If a URL establishment child reports failure, StartupCoordinator treats only
that URL as failed and tries the next URL for the same peer when one remains. A
single URL failure does not stop startup for other peers and does not stop
fallback attempts for that peer. When a child reports that peer root directory
creation failed during a normal run, StartupCoordinator treats that report as a
failed URL attempt.

If every URL for a peer fails, StartupCoordinator marks that peer unreachable
for the run and creates one error-level diagnostic for that peer. The
diagnostic must identify the unreachable peer and must be data returned across
this boundary. StartupCoordinator does not print it.

The startup result contains:

- every reachable peer, identified by the caller's peer identity and role;
- each reachable peer's winning URL;
- the connection handle or effective connection settings returned by the URL
  establishment child for that winning URL;
- every unreachable peer;
- one error-level diagnostic for each unreachable peer;
- a fatal startup status when fewer than two peers are reachable;
- a fatal startup status when the canon peer is unreachable.

The reachable peer set returned by StartupCoordinator excludes every peer whose
URLs all failed. Later startup and run work must use only that reachable peer
set.

The result must make the winning URL invariant clear to later operations:
reachable peer handles returned by this child represent exactly one selected
URL, and later peer work must use that URL instead of re-running fallback
selection.

## Boundaries

StartupCoordinator does not parse command-line text, validate peer arguments,
normalize URLs, decide peer roles, choose the canon peer, or format user output.
Those decisions arrive as structured input from PeerConnections, and
diagnostics leave as structured records.

StartupCoordinator does not create local directories, open SFTP sessions, check
known hosts, choose SFTP credentials, apply SSH handshake timeouts, or apply
SFTP idle keep-alive settings itself. Those behaviors belong to the URL
establishment children. StartupCoordinator only passes through the structured
inputs those children need and records their success or failure.

StartupCoordinator does not exit the process. It reports fatal startup status
when reachability violates the startup rules, and PeerConnections or its caller
owns the actual process exit behavior.

StartupCoordinator does not retry fallback URLs after startup and does not
reselect a different URL for a reachable peer after a winner has been chosen.
Failures from later listing, snapshot, transfer, or dry-run planning operations
do not cause this child to try remaining fallback URLs.

## Invariants

- Startup work is begun for all peers in parallel.
- URLs inside one peer are tried in primary-then-fallback command-line order.
- Each reachable peer has exactly one winning URL for the run.
- A peer's later fallback URLs are not tried after that peer has a winner.
- A peer is unreachable only when every URL for that peer fails at startup.
- Each unreachable peer produces one error-level diagnostic.
- Unreachable peers are excluded from the returned reachable peer set.
- Startup is fatal when fewer than two peers are reachable.
- Startup is fatal when the canon peer is unreachable.
