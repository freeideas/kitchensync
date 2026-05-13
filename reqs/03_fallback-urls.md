# 03_fallback-urls: Fallback URL bracket syntax

## Behavior

Square brackets group multiple URLs into a single peer — different network paths to the same data. URLs are tried in order; the first that connects wins, and the remaining URLs are not attempted. The `+`/`-` prefix attaches to the bracket as a whole. Derived from `sync.md` §"Fallback URLs", `README.md` §"Fallback URLs", and `concurrency.md` §"Connection Establishment".

## $REQ_IDs

- `03.52` — `[url1,url2,...]` is treated as a single peer with the listed URLs as fallback network paths.
- `03.53` — When connecting to a fallback-URL peer, URLs are tried in the order given.
- `03.54` — The first URL that successfully connects is used for the rest of the run; remaining URLs are not tried.
- `03.55` — If every URL in the bracket fails, the peer is unreachable for the run (same handling as a single-URL peer that cannot connect).
- `03.56` — A `+` or `-` prefix on the bracket applies to the whole peer (not to individual URLs inside).
- `03.57` — Per-URL query string settings (`?mc=...`, `?ct=...`, `?ka=...`) attach to individual URLs inside the bracket.

## Notes

Pool keying (user+host) and timeout behavior of the connection attempt are in `03_sftp-pool.md`.
