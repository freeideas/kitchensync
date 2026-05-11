# 02_bracket-fallback: Parse the [u1,u2,...] fallback-URL bracket form

## Behavior

A bracketed peer argument lists multiple URLs that represent the same logical peer in fallback-priority order. `parse_peer_arg` produces a `Peer` with one URL list entry per comma-separated URL inside the brackets, preserving the listed order. The `+`/`-` prefix (if any) decorates the bracket as a whole rather than any individual URL inside. Derived from `SPEC.md` §"Peer-argument parsing" (bracket syntax rules and the prefix-attachment rule).

## $REQ_IDs
- `02.10` — A bracketed `[u1,u2,...]` argument produces a `Peer` whose URL list contains one entry per comma-separated URL, in the order listed.
- `02.11` — A `+` or `-` prefix on a bracketed argument applies to the resulting `Peer` as a whole; the individual `Url` entries inside the bracket do not themselves carry that prefix.
- `02.12` — Whitespace between bracket delimiters, commas, and the listed URLs is not significant; the URLs parse the same way with or without it.
