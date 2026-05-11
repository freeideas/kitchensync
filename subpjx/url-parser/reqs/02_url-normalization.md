# 02_url-normalization: Produce the canonical identity string for a URL

## Behavior

`normalize_url` reduces a URL to the canonical identity string used everywhere else for peer comparison and lookup. It accepts either a `Url` already produced by `parse_peer_arg` or a raw string (which it parses first). The normalization steps are applied in the order listed in the spec. Derived from `SPEC.md` §"URL normalization".

## $REQ_IDs
- `02.20` — `normalize_url` lowercases the URL scheme in the returned identity string.
- `02.21` — `normalize_url` lowercases the hostname of an `sftp` URL in the returned identity string.
- `02.22` — `normalize_url` removes a default SFTP port of `22` from the returned identity string when present.
- `02.23` — `normalize_url` collapses consecutive slashes in the path to a single slash in the returned identity string.
- `02.24` — `normalize_url` removes a trailing slash from the path in the returned identity string, except when the entire path is `/`.
- `02.25` — `normalize_url` percent-decodes unreserved characters (per RFC 3986) in the returned identity string.
- `02.26` — `normalize_url` strips the query string from the returned identity string entirely.
- `02.27` — `normalize_url` accepts either a parsed `Url` or a raw string argument and yields the same canonical identity for the same underlying URL in either form.
- `02.28` — Two URLs that differ only in elements removed or canonicalized by the rules above produce the same canonical identity string.

## Notes

Per-URL settings (`mc`, `ct`, `ka`) live in the query string and are intentionally excluded from the identity by bullet 02.26; their parsing is covered in `03_per-url-settings.md`.
