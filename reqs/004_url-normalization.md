# 004_url-normalization: URL identity normalization

## Behavior
This concern derives from `specs/database.md` section "URL Normalization",
`specs/sync.md` sections "Per-URL Settings" and "URL Schemes", and
`plan/url-normalization.md`. It covers how accepted peer paths and URLs are
normalized before comparison or lookup, including file URL conversion, SFTP
username insertion, default port removal, query stripping, path slash cleanup,
host and scheme case, and percent decoding where the specs require it.

## $REQ_IDs
- `004.1` -- A peer argument with no URL scheme is normalized as a `file://` peer URL before peer identity comparison or lookup.
- `004.2` -- A Windows drive path peer argument is normalized as a `file://` peer URL before peer identity comparison or lookup.
- `004.3` -- A relative local peer path is normalized to an absolute `file://` peer URL resolved from the process current working directory before peer identity comparison or lookup.
- `004.4` -- Peer URL schemes are normalized to lowercase before peer identity comparison or lookup.
- `004.5` -- Peer URL hostnames are normalized to lowercase before peer identity comparison or lookup.
- `004.6` -- The default SFTP port `22` is removed from peer URL identity before peer identity comparison or lookup.
- `004.7` -- A non-default SFTP port remains part of peer URL identity after normalization.
- `004.8` -- Consecutive slashes in a peer URL path are collapsed before peer identity comparison or lookup.
- `004.9` -- A trailing slash is removed from a peer URL path before peer identity comparison or lookup.
- `004.10` -- Percent-encoded unreserved characters in a peer URL path are decoded before peer identity comparison or lookup.
- `004.11` -- Percent-encoded reserved characters in a peer URL path remain encoded in peer URL identity after normalization.
- `004.12` -- Query-string parameters are stripped from peer URL identity before peer identity comparison or lookup.
- `004.13` -- An SFTP peer URL with no username is normalized with the current OS username in the peer URL identity.
- `004.14` -- An SFTP peer URL with an explicit username keeps that username in the peer URL identity after normalization.

## Notes
This file covers peer identity, not connection order, transport operations, or
authentication. Connection order and inline SFTP password decoding belong to
`005_peer-connection-and-authentication.md`.
