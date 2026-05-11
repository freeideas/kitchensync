# 02_uri-components: Uri exposes scheme, authority, path, query, fragment

## Behavior

The `Uri` value returned by `parse_uri` exposes the five RFC 3986 §3 components. The `scheme` is preserved in its raw form (the caller decides whether to fold case). `authority`, when present, is itself a record of `userinfo`, `host`, and `port`; `userinfo` further splits into `user` and `password` at the first `:`. The library distinguishes an absent userinfo from a present-but-empty one (`@` with nothing before it). `path` is always present (possibly empty). `query` and `fragment` are exposed only if present, and the library does not split `query` on `&` or `=` — that is a higher-level concern. Derived from `specs/SPEC.md` "API surface — Parsing" (RFC 3986 §3).

## $REQ_IDs

- `02.10` — When the input has a scheme, the parsed `Uri` exposes it; when absent, the scheme is reported as absent.
- `02.19` — The exposed `scheme` preserves the case of the input (no case folding is applied on access).
- `02.11` — When the input has an authority, the parsed `Uri` exposes it as a record with `userinfo`, `host`, and `port`; otherwise `authority` is reported as absent.
- `02.12` — When `userinfo` is present, it splits into `user` and `password` at the first `:`.
- `02.13` — An empty `userinfo` (the `@` is present but nothing precedes it) is reported as present-but-empty, distinct from absent.
- `02.14` — `port`, when present, is an integer.
- `02.15` — `path` is always present in the parsed `Uri` and may be empty.
- `02.16` — `query` is exposed when present and absent otherwise.
- `02.17` — `fragment` is exposed when present and absent otherwise.
- `02.18` — When `query` is present, its value is the raw component string (not split on `&` or `=`).
