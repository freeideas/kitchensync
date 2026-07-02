# FileUrlConnection:

## Purpose

FileUrlConnection establishes one already-parsed `file://` peer URL during
startup. For this child, establishing a connection means deciding whether the
local peer root can be used for this run, and in a normal run, creating that
root and any missing parents when needed.

This child is the local filesystem URL adapter beneath PeerConnections. It does
not choose among fallback URLs, start peer work in parallel, or decide whether
startup may continue. Its caller uses this child once per `file://` URL attempt
and treats the returned success or failure as the outcome for that URL.

## Responsibilities

FileUrlConnection exposes an operation that accepts:

- an already-validated local peer root path for one `file://` URL;
- the run mode, normal or dry-run;
- any effective connection timeout and SFTP idle keep-alive settings already
  present on the URL or inherited from global settings.

The operation returns either:

- a successful local peer handle that records the peer root path accepted for
  this run; or
- a structured URL-establishment failure for the caller to attach to that URL
  attempt.

Connection timeout and SFTP idle keep-alive values are accepted only so callers
can pass one common URL-establishment input shape. FileUrlConnection must not
use those values to delay, time out, keep alive, retry, or otherwise change
`file://` establishment behavior.

In a normal run, FileUrlConnection checks the local peer root path. If the root
directory already exists, the URL succeeds. If the root path is missing, this
child creates the root directory and all missing parent directories before
returning success. The returned handle is valid only after the directory exists
as a directory.

If directory creation fails in a normal run, this child returns a URL failure.
Creation failure includes any local filesystem error that prevents the root
directory and required parents from existing as directories at the end of the
attempt. The failure must preserve enough structured reason data for
PeerConnections to report that this URL failed, while leaving final stdout
wording to the caller.

In dry-run, FileUrlConnection checks whether the local peer root path already
exists as a directory. If it exists, the URL succeeds and returns a local peer
handle. If the root path or any required parent path is missing, this child
returns a URL failure and creates no directory. Dry-run never creates a peer
root directory or a missing peer root parent through this child.

## Boundaries

FileUrlConnection does not parse command-line peer text, normalize local paths,
convert bare paths to `file://` URLs, validate URL syntax, or decide peer
identity. The caller supplies a local root path that is already the intended
filesystem path for the peer URL.

FileUrlConnection does not open SFTP sessions, perform SSH handshakes, apply
connection timeouts, send keep-alives, verify host keys, or authenticate. Those
settings and behaviors are outside this child and must not affect local file
URL establishment.

FileUrlConnection does not try fallback URLs, record winning URLs, mark peers
unreachable, emit diagnostics, or decide fatal startup status. It reports only
whether this one `file://` URL attempt succeeded or failed.

FileUrlConnection does not perform later transport operations. It does not list
directories, read or write files, create sync-tree directories beyond the peer
root during startup, rename entries, delete entries, set modification times,
recover SWAP state, manage BAK or TMP storage, or update snapshots.

## Invariants

- One call establishes exactly one `file://` URL attempt.
- Normal-run success means the local peer root exists as a directory before the
  success is returned.
- Normal-run root creation failure is reported as a URL failure.
- Dry-run success requires the local peer root to already exist as a directory.
- Dry-run never creates the peer root directory or any missing parent directory.
- Connection timeout and SFTP idle keep-alive settings do not affect
  `file://` establishment.
