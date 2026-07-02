# PeerArgumentParser:

## Purpose

PeerArgumentParser owns the peer-operand part of a non-help KitchenSync
invocation. It turns the ordered peer strings left after global option parsing
into validated peer records that describe peer role, fallback targets, target
kind, SFTP connection fields, and URL-level timeout overrides.

This child does not parse global flags, print help, normalize peer identities,
or connect to peers. It returns structured success or a validation failure for
its caller to report through the command facade.

## Responsibilities

PeerArgumentParser exposes an operation that accepts:

- The ordered peer operands from the command line.
- The already-parsed global connection timeout.
- The already-parsed global idle timeout.
- The current operating-system username supplied by the command facade.

The operation returns either a parsed peer list in command-line order or one
validation failure. A valid non-help invocation has at least two peer operands.
Additional peer operands after the first two are accepted. Fewer than two peer
operands is a validation failure.

Each peer operand may begin with one role marker:

- `+` marks the peer as the canon peer.
- `-` marks the peer as a subordinate peer.
- No marker marks the peer as a normal bidirectional peer.

At most one peer in the parsed list may be canon. More than one canon peer is a
validation failure. Multiple subordinate peers are valid.

A bracketed operand is one peer with multiple fallback targets. The parser
recognizes a bracketed operand after removing one optional leading role marker.
It splits the bracket contents on commas, parses each member with the same
single-target rules, and preserves member order exactly as given. The leading
role marker applies to the whole fallback peer. Role marker characters inside
the brackets are part of the member text and are not parsed as per-member roles.

Each peer target is one of:

- A local file peer when the target is a bare path with no URL scheme.
- A local file peer when the target is a `file://` URL.
- An SFTP peer when the target is an `sftp://` URL.

Local file peers include Unix-style absolute paths, Windows drive paths, and
relative paths. A Windows drive path is local even though it contains a colon.
The parser records the accepted local path or local file URL as a local peer
target; absolute path resolution and file URL identity creation are outside
this child.

For SFTP targets, the parser records:

- Host.
- User name.
- Optional inline password.
- SSH port.
- Remote absolute path.
- URL-level connection timeout.
- URL-level idle timeout.

An SFTP URL in the form `sftp://user@host/path` uses the named user and SSH
port `22`. An SFTP URL in the form `sftp://user@host:port/path` uses the named
port. An SFTP URL in the form `sftp://host/path` uses the supplied current
operating-system username. An SFTP URL in the form
`sftp://user:password@host/path` records the inline password.
Percent-encoded `@` and `:` characters in the password are decoded as password
characters. The path portion of the SFTP URL is recorded as an absolute path
from the remote filesystem root.

The only accepted URL query parameters are `timeout-conn` and `timeout-idle`.
`timeout-conn` is parsed as the connection timeout for that URL.
`timeout-idle` is parsed as the idle keep-alive timeout for that URL. Each value
must be a positive integer; zero, negative numbers, empty strings, fractional
numbers, and non-numeric strings are invalid. A URL-level timeout overrides the
matching global timeout for that URL only; when a timeout query parameter is
absent, the parsed target keeps the matching global timeout. The parser rejects
`max-copies` as a URL query parameter and rejects every URL query parameter
other than `timeout-conn` and `timeout-idle`.

A validation failure owned by this child reports one reason to its caller. The
reason must identify whether the failure is too few peer operands, more than
one canon peer, an unsupported peer URL form, an unsupported query parameter,
or an invalid URL timeout value. The exact user-visible wording belongs to the
command facade.

## Boundaries

PeerArgumentParser does not know the exact help text, exit code, or stdout
format for validation failures. It only reports that validation failed and why.
The command facade is responsible for turning that failure into user-visible
output.

PeerArgumentParser does not parse no-argument help behavior, global options,
global option defaults, verbosity, copy limits, retry counts, retention ages, or
command-line excludes. Those belong to the global argument parser.

PeerArgumentParser does not read the operating-system username itself. The
caller supplies that value so command-facing code controls host-environment
access.

PeerArgumentParser does not normalize peer identities. It does not lowercase
schemes or hosts for identity, remove default ports for identity, strip query
strings from identity, collapse path slashes, remove trailing path slashes,
decode path identity characters, resolve relative local paths, or convert local
paths to `file://` URLs.

PeerArgumentParser does not choose which fallback target to use, connect to
SFTP, authenticate, list files, create directories, or make sync decisions. Its
invariant is that every successful result is a complete syntactic description of
the peer operands, with command-line peer order and fallback target order
preserved.

## Invariants

- Successful parsing preserves peer operand order.
- Successful parsing preserves fallback target order inside each bracketed
  peer.
- Every successful peer has exactly one role: canon, subordinate, or normal.
- Every successful run has at least two peers and no more than one canon peer.
- Every successful SFTP target has a host, user, SSH port, absolute remote
  path, and URL-level timeout settings.
- Every successful URL timeout setting is a positive integer value in seconds.
