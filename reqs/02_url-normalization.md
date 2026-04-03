# URL Normalization and Fallback URLs

URL parsing, normalization rules, and fallback URL grouping.

## $REQ_URL_001: URL Normalization Applied Before Connection
**Source:** ./specs/database.md (Section: "URL Normalization")

URL normalization is applied before any comparison, lookup, or connection attempt -- not just before storage.

## $REQ_URL_002: Scheme and Hostname Lowercased
**Source:** ./specs/database.md (Section: "URL Normalization")

The URL scheme and hostname are lowercased during normalization (e.g., `SFTP://User@Host` becomes `sftp://user@host`).

## $REQ_URL_003: Default Port Removed
**Source:** ./specs/database.md (Section: "URL Normalization")

The default port is removed during normalization (port 22 for SFTP). `sftp://user@host:22/path` normalizes to `sftp://user@host/path`.

## $REQ_URL_004: Path Normalization
**Source:** ./specs/database.md (Section: "URL Normalization")

Consecutive slashes in the path are collapsed and trailing slashes are removed.

## $REQ_URL_005: Bare Paths Converted to file:// URLs
**Source:** ./specs/database.md (Section: "URL Normalization")

Bare paths (no scheme) are converted to `file://` URLs. `file://` URLs are resolved to absolute paths using the working directory at program startup, then symlinks are resolved (OS-canonicalized).

## $REQ_URL_006: Query String Parameters Stripped
**Source:** ./specs/database.md (Section: "URL Normalization")

Query-string parameters (`?mc=5`, `?ct=60`) are stripped from URLs for identity/comparison purposes. They are parsed and applied as per-URL settings before stripping.

## $REQ_URL_007: Percent-Decode Unreserved Characters
**Source:** ./specs/database.md (Section: "URL Normalization")

Unreserved characters that are percent-encoded are decoded during normalization.

## $REQ_URL_008: Fallback URL Bracket Syntax
**Source:** ./README.md (Section: "Fallback URLs")

Multiple URLs for the same peer are grouped with square brackets: `[url1,url2,...]`. URLs are tried in order; the first that connects wins.

## $REQ_URL_009: Prefix on Bracket Group
**Source:** ./specs/concurrency.md (Section: "Fallback URLs")

The `+`/`-` prefix goes on the bracket group, not on individual URLs within the group: `+[url1,url2]` or `-[url1,url2]`.

## $REQ_URL_010: Per-URL Settings Override Global
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Per-URL query-string settings (`mc`, `ct`) override global flag settings for that URL.
