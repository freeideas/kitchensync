# Suggested Decomposition

This file lists standalone components to carve out of kitchensync. The list is a **minimum**, not a maximum — additional standalone components are welcome whenever a piece of functionality can be specified using only spec terminology and external standards (RFCs, well-known abstractions). When in doubt, prefer to carve.

The goal: standalone components do as much of the heavy lifting as possible, so the kitchensync glue mostly orchestrates them. Each component below has a clear external anchor and a self-contained API surface. The detailed contracts (operations, semantics, error cases) live in the spec sections each component implements; this file just names the components and points at their anchors.

## Minimum Suggested Components

### sftp-protocol

The SFTP wire protocol over SSH, with the connection pool described in `concurrency.md`.

**Anchored in:** `draft-ietf-secsh-filexfer` (SFTP), RFC 4253 / 4254 (SSH transport and authentication), `sync.md` §"Peer Transports" (the operation surface), `sync.md` §"Authentication", `concurrency.md` §"Connection Pool (SFTP)".

### gitignore-matcher

Compiles gitignore-style pattern text into a path matcher; pure function, no I/O.

**Anchored in:** the gitignore pattern syntax (well-documented external standard at git-scm.com), `ignore.md` §"Pattern Format" and §"Hierarchy".

### url-parser

Parses kitchensync's URL grammar (peer prefixes, fallback brackets, per-URL query settings, scheme dispatch) into a structured peer description, and normalizes URLs into the canonical identity used everywhere else.

**Anchored in:** RFC 3986 (URI Generic Syntax), `sync.md` §"Peers" / §"Fallback URLs" / §"Per-URL Settings" / §"URL Schemes", `database.md` §"URL Normalization".

**Implement url-parser as a single unit. Do not carve inside it.** RFC 3986 parsing, RFC 8089 file URI handling, percent-encoding, normalization, and the kitchensync-specific URL grammar are all internal helpers of url-parser — not separate carve-outs. They are the substance of url-parser itself.

### decision-engine

The entry-classification and per-entry decision logic — pure function over peer states and snapshot rows, no filesystem, no networking, no SQL.

**Anchored in:** `multi-tree-sync.md` §"Entry Classification", §"Decision Rules", §"Directory Decisions", §"Type Conflicts".

### snapshot-db

The per-peer snapshot database — schema, path hashing, timestamp formatting, and the descendant-cascade tombstone mechanic.

**Anchored in:** SQLite (external standard), xxHash64 (external standard), `database.md` (schema, path hashing, timestamp format, tombstones), `multi-tree-sync.md` §"Snapshot Updates" and §"Orphaned Snapshot Rows".

## Glue

Glue is whatever the kitchensync spec asks for that isn't done by the components above. See `aitc/DESIGN.md` §2.8 for what glue legitimately does (orchestration, transport branching, streaming pumps, type marshaling, error mapping) and what it shouldn't (reimplement work that should have been a carve).

## Adding More Components

If during construction it becomes clear that another piece of functionality is amenable to standalone extraction — its contract anchors cleanly in the spec or an external standard, and it can be specified without reference to siblings — carve it out, even if not listed above.
