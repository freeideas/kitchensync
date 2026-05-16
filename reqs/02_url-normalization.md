# 02_url-normalization: Canonical URL identity

## Behavior

URLs are normalized to a canonical form before any comparison or lookup. Two URLs that differ only in case, default port, redundant slashes, trailing slash, missing scheme, percent-encoding of unreserved characters, or per-URL query parameters resolve to the same peer identity. Derived from `database.md` §"URL Normalization" and `sync.md` §"URL Schemes" / §"Per-URL Settings".

## $REQ_IDs

- `02.12` — A bare path argument is normalized to a `file://` URL with an absolute path resolved from the current working directory.
- `02.13` — The scheme and hostname of an SFTP URL are normalized to lowercase.
- `02.14` — A default port (`:22`) on an `sftp://` URL is removed during normalization.
- `02.15` — Consecutive slashes in the path are collapsed during normalization.
- `02.16` — Query-string parameters (e.g. `?mc=5`) are stripped from a URL's identity during normalization.
- `02.17` — An SFTP URL with no username is normalized to include the current OS user as the username.
- `02.32` — A trailing slash on the path is removed during normalization.
- `02.33` — Percent-encoded unreserved characters in a URL are decoded during normalization.
- `02.46` — An `sftp://.../path` peer treats `/path` as an absolute path on the SFTP server's filesystem, not as a path relative to the remote user's home directory.

## Notes

Where URL identity matters in the user-visible behavior: a snapshot row keyed by path is unaffected by trailing slashes or query strings; running the same peer expressed two equivalent ways yields one peer in the group, not two.
