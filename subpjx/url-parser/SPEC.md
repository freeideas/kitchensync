# URL Parser

## Purpose
Parse peer URL text into structured peer descriptions, including prefix modifiers, fallback URL groups, per-URL settings, URL scheme dispatch, and normalized URL identity.

## Public API
Data shapes:

- `ParseContext`: `current_working_directory`, `current_os_user`
- `PeerRole`: `canon`, `subordinate`, or `bidirectional`
- `Peer`: `role`, ordered `urls`
- `PeerUrl`: `scheme`, `user` optional, `password` optional, `host` optional, `port` optional, `path`, `settings`, `normalized_url`
- `PerUrlSettings`: optional `mc`, `ct`, `ka`
- `NormalizedUrl`: canonical URL string used for identity comparison

Operations:

- `parse_peer(text, context) -> Peer`
- `parse_url(text, context) -> PeerUrl`
- `normalize_url(text, context) -> NormalizedUrl`

## Behavior
`parse_peer` accepts a single peer argument. A leading `+` sets `role = canon`; a leading `-` sets `role = subordinate`; no prefix sets `role = bidirectional`.

Square brackets group fallback URLs into one peer. The fallback group preserves URL order. A prefix applies to the whole group.

Bare paths are parsed as `file://` URLs. Local absolute paths, relative paths, and Windows drive paths are accepted as local paths.

Supported schemes are `file` and `sftp`.

`sftp://` URLs may include user, password, host, optional port, and absolute path. If an SFTP URL has no username, `current_os_user` is inserted.

Per-URL query parameters `mc`, `ct`, and `ka` are parsed into `PerUrlSettings`. Query parameters do not affect `normalized_url`.

Normalization:

- Lowercase the scheme and hostname
- Remove default port `22` for SFTP
- Collapse consecutive slashes in the path
- Remove trailing slash from the path
- Convert bare paths to `file://` URLs
- Resolve `file://` URLs to absolute paths from `current_working_directory`
- Percent-decode unreserved characters
- Strip query-string parameters
- Insert `current_os_user` for SFTP URLs with no username

## Errors
Invalid input returns one of:

- `invalid_peer`
- `invalid_url`
- `invalid_fallback_group`
- `invalid_prefix`
- `unsupported_scheme`
- `invalid_port`
- `invalid_percent_encoding`
- `invalid_setting`

Settings `mc`, `ct`, and `ka` must be positive integers when present.

## Anchoring
`Peer`, `PeerRole`, prefix modifiers, fallback groups, per-URL settings, and supported URL schemes are anchored in `sync.md` "Peers", "Fallback URLs", "Per-URL Settings", and "URL Schemes".

`NormalizedUrl` and normalization behavior are anchored in `database.md` "URL Normalization".

`current_working_directory` is anchored in `database.md` "URL Normalization" for resolving `file://` URLs.

`current_os_user` is anchored in `database.md` "URL Normalization" and `sync.md` "URL Schemes" for SFTP URLs without usernames.

URI parsing, percent-encoding, query strings, host, port, userinfo, and path syntax are anchored in RFC 3986.

`file://` URL handling is anchored in RFC 8089.
