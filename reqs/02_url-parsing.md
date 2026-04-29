# 02_url-parsing: Peer URL forms, prefixes, fallback brackets, query strings

## Behavior

Each peer argument on the command line is a URL or local path that may carry a `+`/`-` prefix, may be a square-bracketed group of fallback URLs, and may carry per-URL settings as query-string parameters. Derived from `./specs/sync.md` (`Peers`, `Fallback URLs`, `Per-URL Settings`, `URL Schemes`) and `./specs/help.md`.

## $REQ_IDs
- `02.1` — Bare paths without a scheme (e.g., `/path`, `c:/photos`, `./relative`) are accepted as `file://` peers.
- `02.2` — `sftp://user@host/path` URLs are accepted as remote peers using SSH on port 22.
- `02.3` — `sftp://user@host:port/path` URLs accept a non-standard SSH port.
- `02.4` — `sftp://host/path` URLs (no user) connect using the current OS user.
- `02.5` — A leading `+` on a peer argument marks that peer as canon.
- `02.6` — A leading `-` on a peer argument marks that peer as subordinate.
- `02.7` — A peer argument of the form `[url1,url2,...]` is treated as one peer with those URLs as fallback paths in the listed order.
- `02.8` — `+`/`-` placed on the bracket (e.g., `+[url1,url2]`) applies to the whole peer; the prefix is not required on individual URLs inside the brackets.
- `02.9` — A URL query string `?mc=N` overrides `--mc` for that URL only; `?ct=N` overrides `--ct` for that URL only.
- `02.10` — Multiple per-URL settings combine with `&` (e.g., `?mc=5&ct=60`).
