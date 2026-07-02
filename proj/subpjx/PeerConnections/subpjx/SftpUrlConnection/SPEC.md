# SftpUrlConnection:

## Purpose

SftpUrlConnection establishes one already-parsed `sftp://` peer URL for
startup. It owns the SSH handshake timeout choice for that URL, server host-key
trust through `~/.ssh/known_hosts`, the required SFTP authentication fallback
chain, and remote peer root preparation.

This child is an adapter used by PeerConnections. It decides whether the single
SFTP URL it was given is connected or failed. It does not choose among fallback
URLs, decide whether a peer is reachable, or decide whether startup may
continue.

## Responsibilities

SftpUrlConnection exposes one boundary operation that attempts to establish an
SFTP URL. The operation accepts structured inputs, not command-line text:

- the SFTP endpoint: host, port, username, and remote peer root path;
- the decoded inline password from the URL, when one was present;
- the URL `timeout-conn` value, when one was present;
- the global `--timeout-conn` value;
- the run mode, normal or dry-run;
- the user's home directory;
- the `~/.ssh/known_hosts` path or contents to check;
- the SSH agent socket value, when one is present.

The operation returns either a connected SFTP URL result or a URL failure. A
connected result carries the endpoint, the remote root path, the effective
handshake timeout used for the URL, and the authenticated connection handle or
connection information needed by later SFTP work. A URL failure carries enough
structured reason data for the caller to report why this URL failed while still
allowing the caller to try the peer's next URL.

When the SFTP URL has a `timeout-conn` query setting, SftpUrlConnection uses
that value to bound the SSH handshake. When the URL does not have a
`timeout-conn` query setting, it uses the global `--timeout-conn` value. If the
chosen timeout expires before the SSH handshake completes, this child fails
only the current URL.

SftpUrlConnection verifies the server host key before accepting the connection.
The server host key is trusted only when `~/.ssh/known_hosts` contains a
matching entry for the host and port being contacted. A missing known-hosts
file, missing entry, mismatched entry, or rejected key fails only the current
URL.

SftpUrlConnection tries SFTP authentication credential sources in this exact
order:

1. inline password from the URL;
2. SSH agent;
3. `~/.ssh/id_ed25519`;
4. `~/.ssh/id_ecdsa`;
5. `~/.ssh/id_rsa`.

If a credential source is absent, unavailable, or rejected by the server,
SftpUrlConnection continues with the next source in the chain. Authentication
fails only after every listed source has been tried or skipped as absent.

After connection and authentication succeed, SftpUrlConnection checks the remote
peer root. In a normal run, it creates the remote peer root directory and any
missing parents through SFTP before accepting the URL as connected. If creating
the remote root or one of its missing parents fails, this child treats the URL
as failed. In dry-run, it does not create remote directories; a missing remote
root fails the URL.

## Boundaries

SftpUrlConnection does not parse command-line arguments, normalize URL identity,
insert a default username, decode percent-encoded URL fields, choose a peer's
winning URL, start peer attempts in parallel, evaluate whether at least two
peers are reachable, or decide whether the canon peer is reachable. Those
decisions belong to callers or other PeerConnections children.

SftpUrlConnection does not handle `file://` URLs. Local root preparation and the
rule that SFTP timeout settings do not affect local URLs belong outside this
child.

SftpUrlConnection does not format final user output. It returns structured
success or failure data. The caller decides how to combine URL failures into an
unreachable-peer diagnostic.

SftpUrlConnection does not perform later sync operations such as listing,
snapshot download, file transfer, rename, delete, or modification-time updates.
It may create only the remote peer root directory and missing parents during
startup root preparation.

## Invariants

- One call attempts exactly one SFTP URL.
- The effective handshake timeout is the URL `timeout-conn` value when present
  and the global `--timeout-conn` value otherwise.
- A handshake timeout, untrusted host key, authentication exhaustion, or remote
  root preparation failure fails the current URL only.
- Host-key trust requires a matching `~/.ssh/known_hosts` entry for the
  contacted host and port.
- Authentication credential sources are tried in the specified order, and an
  absent or rejected source does not stop the fallback chain.
- A normal-run success means the remote peer root exists, including any parents
  this child had to create.
- A dry-run call does not create remote directories.
