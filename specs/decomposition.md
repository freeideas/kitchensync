# Decomposition

These are component boundaries that make sense for kitchensync from an
architectural point of view. They are suggestions about useful library seams in
this project, not construction rules.

## SFTP protocol

A component for the SFTP protocol over SSH, including authentication, sessions,
directory listing, file reads and writes, and the connection pooling behavior
described in `concurrency.md`.

This component is anchored in the SFTP draft, RFC 4253, RFC 4254, `sync.md`
"Peer Transports" and "Authentication", and `concurrency.md` "Connection Pool
(SFTP)".

## Gitignore matcher

A component for compiling gitignore-style pattern text into path matchers.

This component is anchored in gitignore pattern syntax and `ignore.md` "Pattern
Format" and "Hierarchy".

## URL parser

A component for parsing peer URL operands, including peer prefixes, fallback
groups, per-URL query settings, supported schemes, and canonical URL identity.

This component is anchored in RFC 3986, RFC 8089, `sync.md` "Peers", "Fallback
URLs", "Per-URL Settings", and "URL Schemes", and `database.md` "URL
Normalization".

## Decision engine

A component for the pure per-entry decision logic: classifying peer state
against snapshots, choosing the authoritative state for a path, and returning
the actions each active peer should take.

This component is anchored in `multi-tree-sync.md` "Entry Classification",
"Decision Rules", "Directory Decisions", and "Type Conflicts".

## Snapshot database

A component for the per-peer snapshot database: schema, path identifiers,
timestamp storage, row lookup, presence updates, tombstones, and descendant
cascade behavior.

This component is anchored in SQLite, xxHash64, `database.md`, and
`multi-tree-sync.md` "Snapshot Updates" and "Orphaned Snapshot Rows".
