# 01_url-normalization: URL normalization to canonical identity

## Behavior

Peer URLs are normalized to a canonical form before any comparison or lookup. The normalized form is the peer's identity used for snapshot lookup and pool keying. Derived from `specs/database.md` §"URL Normalization" and `specs/sync.md` §"URL Schemes".

## $REQ_IDs
- `01.12` — A bare path (e.g., `c:/photos`, `./data`, `/var/photos`) is converted to a `file://` URL.
- `01.13` — A relative `file://` path is resolved to an absolute path from the current working directory.
- `01.14` — Scheme and hostname are lowercased in the normalized URL.
- `01.15` — The SFTP default port `22` is removed when explicit (e.g., `SFTP://Host:22/path/` → `sftp://host/path`).
- `01.16` — Consecutive slashes in the URL path are collapsed (e.g., `sftp://host//docs` → `sftp://host/docs`).
- `01.17` — A trailing slash on the URL path is removed in the normalized form.
- `01.18` — Percent-encoded unreserved characters in the URL are decoded during normalization.
- `01.19` — Query-string parameters (e.g., `?mc=5`) are stripped from the normalized URL — they are not part of identity.
- `01.20` — An SFTP URL with no username is normalized to include the current OS user (e.g., `sftp://host/path` → `sftp://ace@host/path` when running as `ace`).

## Notes
Two URL strings that normalize to the same canonical form refer to the same peer. For SFTP, the canonical user+host identity is also the pool key — see `03_sftp-pool.md`.
