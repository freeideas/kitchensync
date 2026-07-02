# PeerArgumentParser:

## Purpose

PeerArgumentParser owns the peer operand part of a non-help KitchenSync
invocation. It turns the ordered peer strings left after global option parsing
into validated peer descriptions.

The parsed descriptions preserve command-line order and state each peer's role,
fallback locations, local or SFTP target form, SFTP connection fields, and
per-URL timeout settings. This child reports validation failures to its caller;
it does not print help text or write output.

## Responsibilities

PeerArgumentParser exposes one parsing operation. The operation accepts:

- The ordered peer operands from the command line.
- The parsed global connection timeout in seconds.
- The parsed global idle timeout in seconds.
- The current operating-system username.

The operation returns either a parsed peer list or one validation failure. A
valid non-help invocation has at least two peer operands. Additional peer
operands after the first two are accepted. Fewer than two peer operands is a
validation failure.

Each peer operand may begin with one role marker:

- `+` marks the peer as the canon peer.
- `-` marks the peer as a subordinate peer.
- No marker marks the peer as a normal bidirectional peer.

At most one peer in the parsed list may be canon. More than one canon peer is a
validation failure. Multiple subordinate peers are valid.

A bracketed operand is one peer with multiple fallback targets. The parser
recognizes the bracket after reading the one optional leading role marker. A
leading `+` before the opening bracket marks the whole fallback peer as canon.
A leading `-` before the opening bracket marks the whole fallback peer as
subordinate. The role marker for a bracketed peer is determined only by the
character before the opening bracket.

The parser splits bracket contents on commas, parses each member with the same
single-target rules, and preserves member order. Local paths and SFTP URLs are
valid fallback locations. Role marker characters inside the brackets are part
of the member text and are not parsed as per-member roles.

Each peer target is one of:

- A local file peer when the target is a bare path with no URL scheme.
- A local file peer when the target is a `file://` URL.
- An SFTP peer when the target is an `sftp://` URL.

Local file peers include Unix-style absolute paths, Windows drive paths, and
relative paths. A path with no URL scheme is treated as a local file peer. A
Windows drive path is local even though it contains a colon. The parser records
the accepted local path or local file URL as a local peer target; absolute path
resolution and local identity URL creation are outside this child.

For SFTP targets, the parser records:

- Host.
- User name.
- Optional inline password.
- SSH port.
- Remote absolute path.
- Effective connection timeout for that URL.
- Effective idle timeout for that URL.

An SFTP URL in the form `sftp://user@host/path` uses the named user and SSH
port `22`. An SFTP URL in the form `sftp://user@host:port/path` uses the named
port. An SFTP URL in the form `sftp://host/path` uses the supplied current
operating-system username. An SFTP URL in the form
`sftp://user:password@host/path` records the inline password.
Percent-encoded `@` and `:` characters in the password are decoded as password
characters. The path portion of the SFTP URL is recorded as an absolute path
from the remote filesystem root.

The only accepted URL query parameters on peer URLs are `timeout-conn` and
`timeout-idle`. `timeout-conn=N` supplies the connection timeout for that URL.
`timeout-idle=N` supplies the idle keep-alive timeout for that URL. A URL may
combine both parameters when both values are valid. Each value must be a
positive integer; zero, negative numbers, empty strings, fractional numbers,
and non-numeric strings are invalid.

A URL-level `timeout-conn` value overrides the global connection timeout for
that URL only. A URL-level `timeout-idle` value overrides the global idle
timeout for that URL only. When a timeout query parameter is absent, the target
uses the matching global timeout supplied by the caller. The parser rejects
`max-copies` as a URL query parameter and rejects every URL query parameter
other than `timeout-conn` and `timeout-idle`.

A validation failure owned by this child reports one reason to its caller. The
reason must identify whether the failure is too few peer operands, more than
one canon peer, an unsupported peer target, an unsupported query parameter, or
an invalid URL timeout value. The command facade owns the exact user-visible
wording, help text, exit code, and stdout-only reporting.

## Boundaries

PeerArgumentParser does not know the exact help text, exit code, or stdout
format for validation failures. It only reports that validation failed and why.
The command facade is responsible for turning that failure into user-visible
output.

PeerArgumentParser does not parse no-argument help behavior, global options,
global option defaults, verbosity, copy limits, retry counts, retention ages, or
command-line excludes. Those belong to the global argument parser.

PeerArgumentParser does not read the operating-system username itself. The
caller supplies that value.

PeerArgumentParser does not normalize peer identities. It does not lowercase
schemes or hosts for identity, remove default ports for identity, strip query
strings from identity, collapse path slashes, remove trailing path slashes,
decode path identity characters, resolve relative local paths, or convert local
paths to `file://` URLs.

PeerArgumentParser does not choose which fallback target to use, connect to
SFTP, authenticate, list files, create directories, or make sync decisions. Its
invariant is that every successful result is a complete syntactic description of
the peer operands.

## Invariants

- Successful parsing preserves peer operand order.
- Successful parsing preserves fallback target order inside each bracketed
  peer.
- Every successful peer has exactly one role: canon, subordinate, or normal.
- Every successful run has at least two peers and no more than one canon peer.
- Every successful SFTP target has a host, user, SSH port, absolute remote
  path, and URL-level timeout settings.
- Every successful URL timeout setting is a positive integer value in seconds.
