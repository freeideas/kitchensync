# Parse and normalize kitchensync peer URLs

## Purpose
Turn raw command-line peer arguments — strings like `+c:/photos`, `-/mnt/usb`, or `+[sftp://host/path?mc=5,sftp://nas.vpn/path]` — into structured peer descriptions, and reduce any URL to the canonical identity string used everywhere else for comparison and lookup. The kitchensync glue calls this component once at startup to interpret argv; the snapshot layer and the transport dispatcher call its normalization function to key peers by identity. This implements `sync.md` §"Peers" / §"Fallback URLs" / §"Per-URL Settings" / §"URL Schemes" and `database.md` §"URL Normalization".

## API surface

### Peer-argument parsing
`parse_peer_arg(s: string) -> Peer` — accepts one positional command-line argument and returns a structured peer description.

The returned `Peer` has:
- A `prefix` of one of three kinds: `canon` (the argument started with `+`), `subordinate` (`-`), or `normal` (no prefix). At most one `+` is the caller's invariant, not this component's.
- A non-empty ordered list of `Url` records, in fallback-priority order. A bare URL argument yields a single-element list; a bracketed `[u1,u2,...]` argument yields one entry per comma-separated URL. The `+`/`-` prefix attaches to the bracket as a whole, not to URLs inside.
- Each `Url` record carries: the canonical identity string (see normalization below), the scheme (`file` or `sftp`), the user / host / port / password components for `sftp` URLs, the absolute path component, and any per-URL settings parsed from the query string. Recognized settings are `mc` (positive integer), `ct` (positive integer), `ka` (positive integer); each is optional and unset settings are reported as absent (so callers can apply globals).

Rules the parser enforces:
- Bare paths with no scheme — including drive-letter paths like `c:\foo` and `./relative` — are treated as `file://` URLs. Relative paths are resolved against the process's current working directory at parse time.
- The bracket syntax `[u1,u2,...]` may appear only at the top level of the argument and contains at least one URL. Commas inside the brackets separate URLs; whitespace is not significant.
- For `sftp://` URLs, an empty userinfo component is filled in with the current OS user. Percent-encoded characters in userinfo and path are decoded for the structured components but the identity string preserves the canonical encoding (see normalization).
- Unsupported schemes, malformed bracket syntax, unrecognized query parameters, non-positive integer settings, and other parse errors are reported as a structured `ParseError` value containing a short human-readable message identifying which argument and which sub-piece failed. The component does not write to stdout/stderr — error reporting is the caller's job.

### URL normalization
`normalize_url(u: Url | string) -> string` — returns the canonical identity string for a URL. Accepts either a `Url` produced by `parse_peer_arg` or a raw string (the raw form is parsed first). The normalization rules, applied in order:
- Lowercase the scheme and hostname.
- Remove the default SFTP port (22) if present.
- Collapse consecutive slashes in the path.
- Remove a trailing slash from the path (except when the path is exactly `/`).
- Bare paths (no scheme) become `file://` URLs.
- For `file://` URLs, resolve the path to an absolute filesystem path.
- Percent-decode unreserved characters per RFC 3986.
- Strip the query string entirely (per-URL settings are not part of the identity).
- For `sftp://` URLs with no userinfo, insert the current OS user.

Two URLs that normalize to the same string are the same peer.

### Convenience
`is_file_url(u: Url) -> bool` and `is_sftp_url(u: Url) -> bool` — scheme-dispatch predicates for the caller's transport selection. No transport logic lives in this component.

## Anchoring
- `Url` / scheme / userinfo / host / port / path / query / percent-encoding / unreserved characters: RFC 3986.
- `sftp` scheme, default port 22, host-and-user-based identity: the SFTP scheme commonly understood and the `sync.md` §"URL Schemes" table.
- `file` scheme, bare-path-to-`file://` coercion: the `file` URI scheme (RFC 8089) and `sync.md` §"URL Schemes".
- Bracket fallback grammar, `+`/`-` prefixes, query-string settings `mc`/`ct`/`ka`: `sync.md` §"Peers", §"Fallback URLs", §"Per-URL Settings".
- Normalization rules and examples: `database.md` §"URL Normalization".
- "Current OS user": the host-language facility for the effective process user (e.g., POSIX `getlogin` / `pwd`, Windows `GetUserName`).
- "Current working directory": the host-language facility for the process cwd at parse time.
