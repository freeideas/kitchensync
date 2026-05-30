# 003_peer-addressing: Peer URL parsing and identity

## Behavior
This concern derives from `specs/sync.md` sections "Peers", "Fallback URLs", "Per-URL Settings", and "URL Schemes", `specs/database.md` section "URL Normalization", `specs/concurrency.md` section "Fallback URLs", and `specs/README.md` sections "First Sync", "Add A Peer", and "Fallback Paths". It covers peer operand address syntax, local-path and SFTP URL forms, bracketed fallback URL groups, placement of `+` and `-` prefixes on peer operands, per-URL query settings, URL normalization for peer identity, and rejection of unsupported per-URL settings.

## $REQ_IDs
- `003.1` -- Each `<peer>` command-line argument is parsed as either a URL or a local path identifying one sync target.
- `003.2` -- A peer path with no URL scheme is treated as a local `file://` peer URL.
- `003.3` -- A `file://` peer URL is parsed as a local peer address.
- `003.4` -- Absolute Unix-style paths are accepted as local peer addresses.
- `003.5` -- Windows drive paths are accepted as local peer addresses.
- `003.6` -- Relative paths are accepted as local peer addresses.
- `003.7` -- SFTP peer addresses accept `sftp://user@host/path`.
- `003.8` -- SFTP peer addresses without an explicit port use port `22`.
- `003.9` -- SFTP peer addresses accept `sftp://user@host:port/path`.
- `003.10` -- SFTP peer addresses accept `sftp://host/path` with the current OS user as the username.
- `003.11` -- SFTP peer addresses accept `sftp://user:password@host/path`.
- `003.12` -- SFTP peer addresses accept `%40` and `%3A` in inline passwords so `@` and `:` can be represented there.
- `003.13` -- SFTP peer URL paths are interpreted as absolute paths from the remote filesystem root.
- `003.14` -- Square brackets around multiple comma-separated URLs parse as one peer address with fallback URLs.
- `003.15` -- A bare local path inside a bracketed fallback URL group is treated as a local `file://` fallback URL.
- `003.16` -- A Windows drive path inside a bracketed fallback URL group is treated as a local `file://` fallback URL.
- `003.17` -- A peer operand with no `+` or `-` prefix is parsed as a normal peer.
- `003.18` -- A `+` prefix before an unbracketed peer address marks that peer as the canon peer.
- `003.19` -- A `-` prefix before an unbracketed peer address marks that peer as a subordinate peer.
- `003.20` -- A `+` prefix before a bracketed fallback URL group marks the whole fallback peer as the canon peer.
- `003.21` -- A `-` prefix before a bracketed fallback URL group marks the whole fallback peer as a subordinate peer.
- `003.22` -- A bracketed fallback URL group rejects `+` or `-` prefixes on individual URLs inside the brackets.
- `003.23` -- At most one canon peer prefix is valid in a run.
- `003.24` -- Multiple subordinate peer prefixes are valid in a run.
- `003.25` -- A URL query string accepts `timeout-conn` as a per-URL connection-timeout setting.
- `003.26` -- A URL query string accepts `timeout-idle` as a per-URL idle keep-alive setting.
- `003.27` -- Per-URL query settings in a bracketed fallback URL group are associated only with the URL on which they appear.
- `003.28` -- KitchenSync rejects URL query parameters other than `timeout-conn` and `timeout-idle` with an argument-validation error.
- `003.29` -- URL normalization lowercases the scheme before peer URL comparison or lookup.
- `003.30` -- URL normalization lowercases the hostname before peer URL comparison or lookup.
- `003.31` -- URL normalization removes the default SFTP port `22` before peer URL comparison or lookup.
- `003.32` -- URL normalization collapses consecutive slashes in the path before peer URL comparison or lookup.
- `003.33` -- URL normalization removes a trailing slash from the path before peer URL comparison or lookup.
- `003.34` -- URL normalization converts bare paths with no scheme to `file://` URLs before peer URL comparison or lookup.
- `003.35` -- URL normalization resolves `file://` URLs to absolute paths from the current working directory before peer URL comparison or lookup.
- `003.36` -- URL normalization percent-decodes unreserved characters before peer URL comparison or lookup.
- `003.37` -- URL normalization strips query-string parameters before peer URL comparison or lookup.
- `003.38` -- URL normalization inserts the current OS user into SFTP URLs that omit a username before peer URL comparison or lookup.

## Notes
This category owns how peer operands are parsed, normalized, and compared. It does not own minimum peer counts or global options, fallback connection attempt behavior, transport operation semantics, or the sync decision semantics of canon and subordinate peer roles.
