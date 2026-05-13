# 03_identity-normalization: Canonical identity computation

## Behavior

Every `ParsedUrl` carries an `identity` string computed by a fixed canonicalization procedure: lowercase the scheme; for sftp, lowercase the host and insert the caller-supplied `default_user` when userinfo is absent; remove the default port (22 for sftp); collapse consecutive slashes in the path; remove a trailing slash without reducing below `/`; percent-decode unreserved characters per RFC 3986 §2.3; drop the query string; and for `file://` URIs, resolve relative paths against the caller-supplied `cwd`. Derived from SPEC.md section "Normalization" and the worked "Examples".

## $REQ_IDs

- `03.12` — The `identity` scheme is lowercased (e.g. `SFTP://...` produces an identity that begins with `sftp://`).
- `03.13` — For sftp identities the host is lowercased (e.g. `sftp://Host/path` → `...host/path`).
- `03.14` — For an sftp URL whose userinfo is absent, the identity contains `default_user` as the user (e.g. `sftp://host/path` with `default_user=ace` → `sftp://ace@host/path`).
- `03.15` — An sftp port equal to the default (`22`) is omitted from the identity.
- `03.16` — Consecutive slashes in the identity path are collapsed (e.g. `sftp://host//docs` produces a path with a single `/` before `docs`).
- `03.17` — A trailing slash is removed from the identity path, but a path consisting of only `/` is left as `/`.
- `03.18` — Unreserved characters in the identity are percent-decoded per RFC 3986 §2.3.
- `03.19` — The query string is not part of the `identity`; it is preserved on `ParsedUrl.query` instead.
- `03.20` — For a `file://` URL produced from a relative bare path, the identity path is the absolute resolution against `cwd`.

## Notes

Two URLs whose `identity` strings are equal name the same target — that is the operational use of the canonical form. Example anchors: `"SFTP://Host:22/path/?mc=5"` with `default_user=ace` → identity `sftp://ace@host/path`; `sftp://host//docs/` with `default_user=ace` → identity `sftp://ace@host/docs`.
