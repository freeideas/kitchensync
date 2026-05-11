# 02_bare-url-argument: Parse a single non-bracketed URL argument

## Behavior

When the argument (after stripping any `+`/`-` prefix) is a single URL — not the bracket form — `parse_peer_arg` returns a `Peer` whose URL list has exactly one entry. Derived from `SPEC.md` §"Peer-argument parsing" ("A bare URL argument yields a single-element list").

## $REQ_IDs
- `02.1` — A non-bracketed URL argument produces a `Peer` whose URL list contains exactly one `Url` entry, corresponding to the parsed argument.
