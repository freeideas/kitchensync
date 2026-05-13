# 03_query-parameters: Recognized query string parameters

## Behavior

Each URL inside a group may carry a query string whose only recognized parameter names are `mc`, `ct`, and `ka`. Each recognized parameter requires a positive integer value. Recognized parameters are exposed on `ParsedUrl.query`; unrecognized parameter names and malformed values are rejected. Derived from SPEC.md section "Grammar" (query-parameter table) and the rejection list in "API surface".

## $REQ_IDs

- `03.7` — A URL with `?mc=N` for positive integer `N` is accepted and exposes the value at `ParsedUrl.query["mc"]`.
- `03.8` — A URL with `?ct=N` for positive integer `N` is accepted and exposes the value at `ParsedUrl.query["ct"]`.
- `03.9` — A URL with `?ka=N` for positive integer `N` is accepted and exposes the value at `ParsedUrl.query["ka"]`.
- `03.10` — A URL carrying any query parameter name outside `{mc, ct, ka}` is rejected.
- `03.11` — A recognized query parameter whose value is not a positive integer is rejected.

## Notes

The meaning of `mc`, `ct`, `ka` is opaque to the parser — only the syntax of each value is validated.
