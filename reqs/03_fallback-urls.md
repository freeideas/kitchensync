# 03_fallback-urls: Bracketed fallback URLs for one peer

## Behavior

Square-bracket syntax groups multiple URLs into a single peer with one shared snapshot — different network paths to the same data. URLs are tried in order; the first that connects wins. Derived from `specs/sync.md` §"Fallback URLs", `specs/concurrency.md` §"Fallback URLs", and `specs/help.md`.

## $REQ_IDs
- `03.18` — `[url1,url2,...]` on the command line groups the listed URLs into a single peer with one shared snapshot.
- `03.19` — For a bracketed peer, URLs are tried in order and the first that successfully connects wins; remaining URLs are not tried in that connection attempt.
- `03.20` — The `+` and `-` prefix modifiers may be placed on a bracketed group (`+[...]`, `-[...]`) but not on individual URLs inside the brackets.
