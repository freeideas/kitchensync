# Peer URL Parser

## Purpose
Parse one peer URL text into a structured `PeerUrl`, including supported scheme handling, per-URL settings, and normalized URL identity.

## Public API
Data shapes:

- `ParseContext`: `current_working_directory`, `current_os_user`
- `PeerUrl`: `scheme`, `user` optional, `password` optional, `host` optional, `port` optional, `path`, `settings`, `normalized_url`
- `PerUrlSettings`: optional `mc`, `ct`, `ka`
- `NormalizedUrl`: canonical URL string used for identity comparison

Operations:

- `parse_url(text, context) -> PeerUrl`

## Behavior
`parse_url` accepts URL text for supported schemes `file` and `sftp`.

Bare paths are parsed as `file://` URLs. Local absolute paths, relative paths, and Windows drive paths are accepted as local paths.

`sftp://` URLs may include user, password, host, optional port, and absolute path. If an SFTP URL has no username, `current_os_user` is inserted.

Per-URL query parameters `mc`, `ct`, and `ka` are parsed into `PerUrlSettings`. Query parameters do not affect `normalized_url`.

`normalized_url` is the canonical URL string for identity comparison after normalization:

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

- `invalid_url`
- `unsupported_scheme`
- `invalid_port`
- `invalid_percent_encoding`
- `invalid_setting`

Settings `mc`, `ct`, and `ka` must be positive integers when present.

## Anchoring
`PeerUrl`, `PerUrlSettings`, and supported URL schemes are anchored in `sync.md` "Per-URL Settings" and "URL Schemes".

`NormalizedUrl` and normalization behavior are anchored in `database.md` "URL Normalization".

`current_working_directory` is anchored in `database.md` "URL Normalization" for resolving `file://` URLs.

`current_os_user` is anchored in `database.md` "URL Normalization" and `sync.md` "URL Schemes" for SFTP URLs without usernames.

URI parsing, percent-encoding, query strings, host, port, userinfo, and path syntax are anchored in RFC 3986.

`file://` URL handling is anchored in RFC 8089.
