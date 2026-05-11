# 03_per-url-settings: Parse mc/ct/ka query-string settings on each URL

## Behavior

Each URL inside a peer argument may carry per-URL overrides for global settings via its query string. The parser recognizes three settings — `mc`, `ct`, and `ka` — each a positive integer. They are all optional, and a setting absent from a URL's query string is reported as unset so the caller can apply a global default rather than mistake a missing setting for a chosen one. Derived from `SPEC.md` §"Peer-argument parsing" (the `Url` settings list and the "unset settings are reported as absent" rule).

## $REQ_IDs
- `03.20` — A `?mc=N` query parameter on a URL sets that URL's parsed `mc` setting to the positive integer N.
- `03.21` — A `?ct=N` query parameter on a URL sets that URL's parsed `ct` setting to the positive integer N.
- `03.22` — A `?ka=N` query parameter on a URL sets that URL's parsed `ka` setting to the positive integer N.
- `03.23` — A URL whose query string omits one of `mc`, `ct`, or `ka` reports that setting as unset (absent) on the parsed `Url`, distinct from any defaulted value.
