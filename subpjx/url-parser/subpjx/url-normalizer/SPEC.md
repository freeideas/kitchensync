# URL Normalizer

## Purpose
Convert URL text into a canonical `NormalizedUrl` string used for identity comparison.

## Public API
Data shapes:

- `ParseContext`: `current_working_directory`, `current_os_user`
- `NormalizedUrl`: canonical URL string used for identity comparison

Operations:

- `normalize_url(text, context) -> NormalizedUrl`

## Behavior
`normalize_url` accepts URL text for supported schemes `file` and `sftp`.

Bare paths are treated as `file://` URLs. Local absolute paths, relative paths, and Windows drive paths are accepted as local paths.

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

- `invalid_url`
- `unsupported_scheme`
- `invalid_port`
- `invalid_percent_encoding`

## Anchoring
`NormalizedUrl` and normalization behavior are anchored in `database.md` "URL Normalization".

`current_working_directory` is anchored in `database.md` "URL Normalization" for resolving `file://` URLs.

`current_os_user` is anchored in `database.md` "URL Normalization" and `sync.md` "URL Schemes" for SFTP URLs without usernames.

URI parsing, percent-encoding, query strings, host, port, userinfo, and path syntax are anchored in RFC 3986.

`file://` URL handling is anchored in RFC 8089.
