# 02_serialize-uri: serialize_uri renders Uri back to a string and round-trips

## Behavior

`serialize_uri` renders a `Uri` value back to its RFC 3986 §5.3 string form. For any well-formed input, parsing then serializing reproduces the original string. Derived from `specs/SPEC.md` "API surface — Serialization" (RFC 3986 §5.3).

## $REQ_IDs

- `02.20` — `serialize_uri` renders a `Uri` back to its RFC 3986 §5.3 string form.
- `02.21` — For any well-formed input, `parse_uri` followed by `serialize_uri` reproduces the original string.
