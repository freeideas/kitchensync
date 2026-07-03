# LocalTransport:

## Purpose

LocalTransport owns access to peer roots reached through `file://` URLs and
bare path peer URLs. It establishes a connected local root for a winning local
candidate URL, then performs peer file operations against paths relative to that
root using host filesystem calls.

The sync logic must see the same transport behavior for a local peer that it
would see through any other peer transport surface. LocalTransport is the
scheme-specific implementation for local files only; it does not decide which
peer URL wins, which files should sync, or how copy staging is sequenced.

## Responsibilities

- Treat a bare path peer URL as local filesystem access once the command-line
  and URL layers have identified it as a local peer.
- Establish a local peer connection without applying connection timeout or
  idle keep-alive settings. Global timeout settings and per-URL
  `timeout-conn` or `timeout-idle` query settings must not delay, cancel, or
  otherwise affect local connection establishment.
- In a normal run, create a missing local peer root and any missing parent
  directories before reporting that the local URL connected successfully.
- If creating the local root or its parents fails in a normal run, report the
  candidate URL as failed for startup. The failure must be reported through the
  same startup result shape used for other failed candidate URLs.
- Keep a connected local root handle for the selected URL. Every later
  operation receives a relative peer path and resolves it under that root.
- Provide local filesystem implementations for the peer operations exposed by
  the shared transport surface: directory listing, metadata lookup, streaming
  read, streaming write, rename to a non-existing destination, file deletion,
  empty directory deletion, directory creation with parents, and modification
  time updates.
- Preserve filenames exactly as the host filesystem reports them.
- Return local operation failures using only the transport-neutral categories:
  `not found`, `permission denied`, and `I/O error`.

## Boundaries

LocalTransport does not parse command-line peer arguments, normalize URL
identity, strip query strings, group fallback URLs, or choose the winning URL
among candidates. The startup coordinator owns candidate ordering and reachable
peer rules; LocalTransport only answers whether one local candidate can be
connected and supplies the connected root when it can.

LocalTransport does not implement SSH, SFTP, host-key verification,
authentication, network handshakes, network timeouts, or idle keep-alives.
Those settings are ignored for local connection establishment because they are
not local filesystem behavior.

LocalTransport does not own sync decisions, copy scheduling, retries, progress
output, snapshot database contents, SWAP/TMP/BAK policy, or dry-run policy for
peer mutations. Callers decide when a peer operation may be attempted.

LocalTransport assumes relative paths crossing its boundary have already been
validated by the path-format and transport-surface layers. It must not accept a
transport-specific escape hatch that lets a caller switch roots after startup.
For a connected local peer, the connected root remains the only base for all
root-relative operations during that run.
