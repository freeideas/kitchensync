# 02_char-class: character-class predicates

## Behavior

The library exposes the RFC 3986 §2.2/§2.3 character-class predicates so callers can validate or classify input without parsing. Derived from `specs/SPEC.md` "API surface — Character classification" (RFC 3986 §2.2, §2.3).

## $REQ_IDs

- `02.50` — `is_unreserved(c)` returns true exactly for characters in the RFC 3986 `unreserved` class.
- `02.51` — `is_reserved(c)` returns true exactly for characters in the RFC 3986 `reserved` class (= `gen-delims ∪ sub-delims`).
- `02.52` — `is_gen_delim(c)` returns true exactly for characters in the RFC 3986 `gen-delims` class.
- `02.53` — `is_sub_delim(c)` returns true exactly for characters in the RFC 3986 `sub-delims` class.
