# 04_parse-errors: Report parse failures as structured ParseError values

## Behavior

Failures during peer-argument parsing are returned as a structured `ParseError` value rather than as a successfully-parsed `Peer`. The error carries a short human-readable message identifying which argument and which sub-piece failed, so the caller can compose a useful diagnostic for the user. Derived from `SPEC.md` §"Peer-argument parsing" (the `ParseError` enumeration and message contract).

## $REQ_IDs
- `04.1` — A peer argument whose URL uses an unsupported scheme is reported as a `ParseError`.
- `04.2` — A peer argument with malformed bracket syntax is reported as a `ParseError`.
- `04.3` — A peer argument containing an unrecognized query parameter on one of its URLs is reported as a `ParseError`.
- `04.4` — A peer argument with a non-positive integer value for `mc`, `ct`, or `ka` is reported as a `ParseError`.
- `04.5` — A returned `ParseError` carries a short human-readable message identifying which argument and which sub-piece failed.
