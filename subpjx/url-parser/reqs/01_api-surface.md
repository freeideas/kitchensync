# 01_api-surface: Public parser entry points and output shape

## Behavior

The library exposes two pure functions over text: `parse(text, cwd, default_user)` and `normalize(url, cwd, default_user)`. `parse` returns a `TaggedGroup` value containing a `role` discriminator and a non-empty ordered list of `ParsedUrl` records. `normalize` is a convenience wrapper that returns the canonical `identity` string of a single URL expression. The parser performs no filesystem or network access; relative paths are resolved against the caller-supplied `cwd`. Derived from SPEC.md sections "Purpose", "Output structure", and "API surface".

## $REQ_IDs

- `01.1` — `parse(text, cwd, default_user)` returns a `TaggedGroup` with a `role` and an ordered list of `ParsedUrl` entries in `urls`.
- `01.2` — `normalize(url, cwd, default_user)` returns the canonical `identity` string for a single-URL expression that carries no role tag and no bracket group.
- `01.3` — For any single URL `u`, `normalize(u, cwd, du)` equals `parse(u, cwd, du).urls[0].identity`.
- `01.4` — Empty input is rejected.

## Notes

Errors are reported through the host language's idiomatic mechanism (exception, error result, etc.); only the fact that the input is rejected is part of the contract.
