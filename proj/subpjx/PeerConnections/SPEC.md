# PeerConnections:

## Purpose

PeerConnections establishes the set of peers that are usable at startup. It
receives already-validated peer definitions from the caller, starts every
peer's connection work in parallel, chooses one winning URL for each reachable
peer, and reports which peers remain available for the rest of the run.

The child is responsible for the connection work that can mutate peer roots:
creating missing peer root directories in normal runs and rejecting missing
roots in dry-run. It also owns SFTP SSH handshake timeout handling, host-key
trust checks, and credential fallback.

## Responsibilities

PeerConnections exposes one startup operation. The operation accepts:

- the ordered peer list from the command line;
- each peer's role, including whether it is the canon peer;
- each peer's primary URL followed by fallback URLs in command-line order;
- parsed per-URL connection settings, including `timeout-conn` and
  `timeout-idle`;
- global connection settings, including `--timeout-conn` and
  `--timeout-idle`;
- the run mode, normal or dry-run;
- the known local environment needed for SFTP authentication, including the
  home directory, `~/.ssh/known_hosts`, and SSH agent socket value.

The operation returns a startup result containing:

- reachable peer handles, preserving the caller's peer identity and role;
- the winning URL and effective SFTP connection settings for each reachable
  peer;
- unreachable peers with one error-level diagnostic per unreachable peer;
- a fatal startup status when fewer than two peers are reachable or the canon
  peer is unreachable.

For every peer, PeerConnections starts connection establishment without waiting
for other peers to finish. Within one peer, it tries URLs sequentially: primary
URL first, then fallback URLs in command-line order. The first URL whose
connection establishment succeeds becomes that peer's winning URL. After a
winning URL is selected, this child must not try any later fallback URL during
that run.

For a reachable peer, the returned peer handle records the winning URL and must
route all later peer operations through that winning URL. Later operations are
allowed to use the handle or connection information returned by this child, but
they must not re-select among fallback URLs. When the winner is an SFTP URL, the
handle also carries the effective connection timeout and idle keep-alive setting
selected for that URL.

For `file://` URLs, connection establishment is local path preparation. In a
normal run, this child creates the peer root directory and any missing parents
before accepting the URL as connected. In dry-run, it does not create missing
local directories; a missing root makes that URL fail. SFTP connection timeout
and idle keep-alive settings do not affect `file://` URL handling.

For `sftp://` URLs, connection establishment includes opening the TCP/SSH/SFTP
connection, verifying the server host key against `~/.ssh/known_hosts`, and
authenticating. If the URL has a `timeout-conn` query setting, that value bounds
the SSH handshake for that URL before this child tries the next URL. Otherwise,
the global `--timeout-conn` value bounds the handshake. The URL's
`timeout-idle` setting, or the global `--timeout-idle` value when the URL omits
it, is retained with the winning SFTP handle for later SFTP connection
management. After the connection and authentication succeed, this child checks
the remote peer root path. In a normal run, it creates the remote root directory
and any missing parents through SFTP before accepting the URL as connected. In
dry-run, it does not create missing remote directories; a missing root makes
that URL fail.

SFTP host keys are trusted only when `~/.ssh/known_hosts` contains a matching
entry for the server and port being contacted. An unknown, absent, or rejected
host key makes that URL fail.

SFTP authentication tries credential sources in this exact order:

1. inline password from the URL;
2. SSH agent;
3. `~/.ssh/id_ed25519`;
4. `~/.ssh/id_ecdsa`;
5. `~/.ssh/id_rsa`.

If a credential source is absent or the server rejects it, this child continues
with the next source. A URL fails authentication only after every listed source
has been tried or skipped as absent.

If root directory creation fails for a normal-run `file://` or `sftp://` URL,
this child treats that URL as failed and tries the peer's next fallback URL, if
one exists.

The startup result reports every unreachable peer. For each peer whose URLs all
fail, this child produces one error-level diagnostic for the caller to print.
Diagnostics are data crossing this boundary; this child does not own final
stdout formatting.

After all peer attempts finish, this child evaluates startup reachability:

- fewer than two reachable peers is a startup error;
- an unreachable canon peer is a startup error.

The result must distinguish ordinary unreachable peers from fatal startup
errors so the caller can exit with the required status and keep all diagnostics
on stdout.

## Boundaries

PeerConnections does not parse command-line text, validate arguments, normalize
URL identity, choose peer roles, format help text, or own general user output.
Those inputs arrive as structured values, and diagnostics leave as structured
error-level records.

PeerConnections does not download snapshots, recover snapshot SWAP state, decide
subordinate status from snapshot presence, list sync tree directories, retry
directory listings, transfer files, displace files to BAK, upload snapshots, or
perform dry-run planning beyond the root existence rule described here.

PeerConnections does not retry fallback URLs after startup. Once a peer has a
winning URL, that URL is invariant for the rest of the run. If later listing,
snapshot, or transfer work fails, that failure belongs to the later operation
and does not cause fallback URL reselection.

PeerConnections does not create any peer-side directory in dry-run mode. In
normal mode, it may create only the peer root directory and its missing parents
as part of accepting a startup URL.

## Invariants

- Peer startup attempts are begun for all peers in parallel.
- URLs inside one peer are tried in the exact command-line order.
- Each reachable peer has exactly one winning URL for the run.
- Remaining fallback URLs are not tried after a winner is selected.
- All later peer work uses the winning URL returned by this child.
- `file://` establishment ignores connection timeout and SFTP idle settings.
- SFTP host-key rejection, authentication exhaustion, handshake timeout, and
  root creation failure all fail only the current URL, not the whole process by
  themselves.
- A peer is unreachable only when all of its URLs fail at startup.
- The canon peer must be reachable, and at least two peers must be reachable,
  before startup may continue.
