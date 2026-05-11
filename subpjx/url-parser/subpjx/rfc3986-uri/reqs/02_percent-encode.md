# 02_percent-encode: percent_encode encodes outside a named safe class

## Behavior

`percent_encode` percent-encodes its input per RFC 3986 §2.1, leaving characters in the caller-named `CharClass` unencoded. The library provides the §2.2/§2.3 character classes (`unreserved`, `gen_delims`, `sub_delims`, `reserved`) as well as the per-component "allowed" classes implied by §3 (`scheme`, `userinfo`, `host`, `path`, `query`, `fragment`). Derived from `specs/SPEC.md` "API surface — Percent-encoding" (RFC 3986 §2.1, §2.2, §2.3, §3).

## $REQ_IDs

- `02.30` — `percent_encode` encodes its input per RFC 3986 §2.1.
- `02.31` — Characters belonging to the caller-named safe `CharClass` are left unencoded in the output.
- `02.32` — `percent_encode` accepts the `unreserved`, `gen_delims`, `sub_delims`, and `reserved` (= `gen_delims ∪ sub_delims`) classes as the safe class.
- `02.33` — `percent_encode` accepts the per-component allowed classes for `scheme`, `userinfo`, `host`, `path`, `query`, and `fragment` as the safe class.
