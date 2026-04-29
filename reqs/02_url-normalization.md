# 02_url-normalization: Canonical form for URL identity comparisons

## Behavior

URLs are normalized to a canonical form before any comparison or lookup so that equivalent URLs hash to the same identity. Derived from `./specs/database.md` (`URL Normalization`).

## $REQ_IDs
- `02.11` — The scheme is lowercased (e.g., `SFTP://...` → `sftp://...`).
- `02.12` — The hostname is lowercased (e.g., `Host` → `host`).
- `02.13` — The default SFTP port (`22`) is removed from the normalized URL.
- `02.14` — Consecutive slashes in the path are collapsed into one (e.g., `sftp://host//docs/` → `sftp://host/docs`).
- `02.15` — A trailing slash is removed from the normalized path.
- `02.16` — A bare path with no scheme is converted to a `file://` URL.
- `02.17` — `file://` URLs from a relative bare path are resolved to an absolute path against the current working directory.
- `02.18` — Query-string parameters (e.g., `?mc=5`) are stripped from the normalized URL — they are not part of the URL's identity.
- `02.19` — Percent-encoded unreserved characters are decoded during normalization.
