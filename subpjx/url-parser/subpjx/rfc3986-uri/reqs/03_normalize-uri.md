# 03_normalize-uri: normalize_uri applies RFC 3986 §6.2.2 syntax normalization

## Behavior

`normalize_uri` takes a parsed `Uri` and applies RFC 3986 §6.2.2 syntax-based normalization: case folding of the scheme and host, decoding of unreserved percent-encoded characters across the userinfo/host/path/query/fragment components, uppercasing the hex digits of the remaining percent-encoded triplets, and `remove_dot_segments` on the path. The component performs no scheme-based normalization (§6.2.3) — default ports, empty-path-under-authority coercion, and similar scheme-aware rules remain the caller's responsibility. Derived from `specs/SPEC.md` "API surface — Normalization" (RFC 3986 §6.2.2.1, §6.2.2.2, §6.2.2.3, §5.2.4).

## $REQ_IDs

- `03.20` — `normalize_uri` lowercases the scheme.
- `03.21` — `normalize_uri` lowercases the host.
- `03.22` — `normalize_uri` decodes unreserved percent-encoded characters in `userinfo`, `host` (reg-name), `path`, `query`, and `fragment`.
- `03.23` — `normalize_uri` uppercases the hex digits of the remaining percent-encoded triplets it does not decode.
- `03.24` — `normalize_uri` applies the `remove_dot_segments` algorithm to the `path`.

## Notes

Scheme-based normalization (§6.2.3) is explicitly out of scope for this component — the spec calls this out as "the caller's responsibility." This is encoded by the absence of any such bullet here; no separate "does not do X" requirement is needed.
