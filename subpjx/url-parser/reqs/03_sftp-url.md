# 03_sftp-url: sftp:// URL parsing — userinfo defaulting and percent-decoding

## Behavior

For `sftp://` URLs, a missing userinfo component is filled in with the current OS user so that downstream comparisons key peers by `(user, host)` consistently. Percent-encoded characters in userinfo and path are decoded for the parsed `Url`'s structured components so callers see the literal user and path values. (The canonical identity string keeps the canonical RFC 3986 encoding — see `02_url-normalization.md`.) Derived from `SPEC.md` §"Peer-argument parsing" (sftp userinfo and percent-decoding rules) and §"URL normalization" (sftp-no-userinfo rule).

## $REQ_IDs
- `03.10` — An `sftp://` URL whose userinfo component is empty or absent is parsed such that the parsed `Url`'s structured user component is the current OS user.
- `03.11` — Percent-encoded characters in an `sftp://` URL's userinfo are decoded in the parsed `Url`'s structured user component.
- `03.12` — Percent-encoded characters in an `sftp://` URL's path are decoded in the parsed `Url`'s structured path component.
- `03.13` — The canonical identity string returned by `normalize_url` for an `sftp://` URL with empty or absent userinfo contains the current OS user as the user component.
