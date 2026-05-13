# 02_bracket-groups: Comma-separated URL list in brackets

## Behavior

A tagged URL group may be a bracketed list of comma-separated URLs: `[url1,url2,...]`. The list produces one `ParsedUrl` per inner URL in input order, and an optional leading role tag applies to the whole group rather than to any inner URL. Several malformed forms are explicitly rejected. Derived from SPEC.md section "Grammar" and the rejection list in "API surface".

## $REQ_IDs

- `02.5` — A bracket group `[u1,u2,...]` produces a `TaggedGroup` with one `ParsedUrl` per inner URL, preserving input order.
- `02.6` — A leading role tag before a bracket group sets the group's role and does not alter the inner URLs.
- `02.7` — A bracket group that is not closed is rejected.
- `02.8` — A bracket group containing an empty URL is rejected.
- `02.9` — A bracket group containing an inner URL prefixed with a role tag is rejected.

## Notes

The role tag appears at most once and only at the start of the whole expression — never on individual URLs inside a bracket group.
