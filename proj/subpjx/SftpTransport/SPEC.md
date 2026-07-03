# SftpTransport:

## Purpose

SftpTransport owns access to `sftp://` peer URLs. It establishes one SSH/SFTP
session for a candidate URL, verifies the server host key, authenticates with
the required credential fallback chain, applies the SFTP connection timeouts for
that URL, prepares the remote peer root when startup is allowed to mutate peer
state, and returns a connected root handle that uses SSH/SFTP operations for all
later peer file access.

The root coordinator and peer selection logic decide which candidate URL is
being tried and whether a failed URL should be skipped. SftpTransport only
answers whether one `sftp://` candidate can become a connected peer root, and it
provides root-relative SFTP operations through the shared peer transport surface.

## Responsibilities

- Expose a connection operation for one normalized `sftp://` candidate URL. The
  operation receives the parsed user, host, port, remote absolute root path,
  optional inline password, global `timeout-conn`, global `timeout-idle`, any
  per-URL timeout overrides, and whether startup may create the missing remote
  root for a normal run.
- Apply URL query timeout overrides before connecting. A URL `timeout-conn`
  value replaces the global SSH handshake timeout for that URL only. A URL
  `timeout-idle` value replaces the global SFTP idle keep-alive TTL for that URL
  only.
- Bound the SSH handshake by the effective connection timeout. If the handshake
  does not complete before that timeout, report candidate connection failure for
  the current run.
- Verify the presented host key against `~/.ssh/known_hosts` after the SSH
  handshake and before authentication. A matching known host is eligible for
  authentication. An unknown host key is rejected.
- Authenticate in this exact order, continuing after every absent source,
  unusable source, or rejected credential: inline URL password, SSH agent from
  `SSH_AUTH_SOCK`, `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`,
  `~/.ssh/id_rsa`.
- Preserve the key fallback behavior needed for an Ed25519-only server: a URL
  with no inline password, no usable SSH agent, and no accepted RSA key can
  still connect through `~/.ssh/id_ed25519` when that key is accepted.
- Open the SFTP subsystem only after host-key verification and authentication
  succeed.
- During normal startup, create the remote peer root directory and any missing
  parents through SFTP before reporting the URL as connected. If the remote root
  cannot be created, report candidate connection failure for the current run.
- Return a connected SFTP root handle whose operations address paths relative
  to the selected remote root and use SSH/SFTP, not local filesystem access.
- Implement the SFTP side of the shared peer transport surface for the connected
  root: listing, stat, streaming reads, streaming writes, rename to a
  non-existing destination, file deletion, directory creation, empty directory
  deletion, and modification-time setting.
- Convert SFTP connection drops and SFTP timeouts during any connected
  operation to the shared `I/O error` category.

## Boundaries

SftpTransport does not parse the command line, validate URL query names,
normalize URLs, group fallback URLs, choose the winning URL for a peer, decide
whether enough peers remain reachable, print diagnostics, schedule copies, or
run reconciliation. Those behaviors belong to other children and the root
coordinator.

SftpTransport does not own the transport-neutral type vocabulary. It depends on
PeerTransportSurface for the operation shapes and shared error categories used
by sync logic. The SFTP implementation must not expose `ssh2` errors or other
transport-specific failures across that boundary.

SftpTransport does not own snapshot semantics, SWAP/BAK policy, copy retry
policy, fallback retry after startup, or global copy-slot limits. It provides
the root-relative operations those features call. After a URL is connected,
later operation failures stay on that connected SFTP handle and do not trigger
fallback URL selection inside this child.

The remote root path is an invariant of the connected handle. Every operation
must remain within that root by joining the caller's relative peer path to the
stored remote root path. The child preserves filenames exactly as the SFTP
server reports them and leaves cross-peer case handling to higher-level sync
logic.
