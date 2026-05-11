# 02_percent-decode: percent_decode and percent_decode_unreserved

## Behavior

`percent_decode` decodes every `%HH` triplet in its input per RFC 3986 §2.1. `percent_decode_unreserved` is the §6.2.2.2 variant: it decodes only triplets whose decoded byte is in the `unreserved` class, and uppercases the hex digits of the remaining valid triplets that it leaves in place. Both functions report invalid triplets (`%` not followed by two hex digits) as a structured `PercentDecodeError`. Derived from `specs/SPEC.md` "API surface — Percent-encoding" (RFC 3986 §2.1, §6.2.2.2).

## $REQ_IDs

- `02.40` — `percent_decode` decodes every `%HH` triplet in its input per RFC 3986 §2.1.
- `02.41` — `percent_decode` returns a structured `PercentDecodeError` when a `%` is not followed by two hex digits.
- `02.42` — `percent_decode_unreserved` decodes only those `%HH` triplets whose decoded byte is in the `unreserved` class.
- `02.43` — `percent_decode_unreserved` uppercases the hex digits of remaining valid triplets that it leaves in place (e.g., `%2f` becomes `%2F`).
- `02.44` — `percent_decode_unreserved` returns a structured `PercentDecodeError` for invalid triplets.
