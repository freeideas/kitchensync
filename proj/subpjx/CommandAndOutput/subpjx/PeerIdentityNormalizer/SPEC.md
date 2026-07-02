# PeerIdentityNormalizer:

## Purpose

PeerIdentityNormalizer owns the canonical identity form used when KitchenSync
compares peer targets or looks up stored peer state. It accepts already-parsed
local file peers and SFTP peer URLs, applies the required identity
normalization rules, and returns a normalized peer URL identity.

This child does not decide whether a command-line operand is valid. It works
after peer argument parsing has accepted the operand and before any comparison
or lookup treats two peer targets as the same or different.

## Responsibilities

PeerIdentityNormalizer exposes an operation that accepts one parsed peer target,
the process current working directory, and the current operating-system
username. The target is either a local path target or an SFTP URL target. The
operation returns the normalized peer URL identity that all later comparison and
lookup code must use.

For local file targets, the normalizer converts the target into a `file://`
identity URL. A peer argument with no URL scheme becomes a `file://` peer URL.
A Windows drive path is treated as a local path and also becomes a `file://`
peer URL. A relative local path is resolved against the supplied process
current working directory before the `file://` identity URL is built.

For every peer URL identity, the normalizer lowercases the URL scheme. For peer
URLs with a hostname, it lowercases the hostname. SFTP URLs with the default
port `22` have that port removed from identity. SFTP URLs with any non-default
port keep that port in identity.

For peer URL paths, the normalizer collapses consecutive slash characters into
one slash and removes a trailing slash from the identity path. It decodes
percent-encoded unreserved path characters for identity and leaves
percent-encoded reserved path characters encoded. Query-string parameters are
not part of peer identity and are stripped from the returned identity URL.

For SFTP peer URLs, the normalizer inserts the supplied current
operating-system username when the parsed URL has no username. When the parsed
URL has an explicit username, the identity keeps that username unchanged.

The operation reports only normalization failures that prevent it from forming
a peer URL identity from an already-accepted target, such as an absolute local
path that cannot be represented as a `file://` URL. It does not write to stdout
or stderr and does not format command validation errors.

The invariant of this child is that every successful result is the only peer
URL identity form used outside the parser for equality, duplicate detection,
snapshot lookup, or any other peer identity lookup.

## Boundaries

PeerIdentityNormalizer does not parse raw command-line arguments, peer role
markers, fallback groups, global options, per-URL timeout query parameters, or
inline SFTP passwords. Those belong to the argument parsing children.

PeerIdentityNormalizer does not choose a fallback target, connect to SFTP,
authenticate, create peer directories, list files, compare file metadata, make
sync decisions, update snapshots, or report progress. It only returns
normalized identity values for accepted local and SFTP peer targets.

PeerIdentityNormalizer does not preserve query-string settings in identity.
Callers that need connection timeout settings must keep those parsed settings
separately from the normalized identity returned by this child.
