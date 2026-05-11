# 03_path-utilities: remove_dot_segments and merge_paths

## Behavior

The library exposes the RFC 3986 path algorithms as standalone utilities so callers can normalize a path without round-tripping through `Uri`, and so callers that perform their own reference resolution can compose them. Derived from `specs/SPEC.md` "API surface — Path utilities" (RFC 3986 §5.2.3, §5.2.4).

## $REQ_IDs

- `03.10` — `remove_dot_segments(path)` implements the RFC 3986 §5.2.4 algorithm in isolation on a path string.
- `03.11` — `merge_paths(base_path, ref_path, base_has_authority)` implements the RFC 3986 §5.2.3 path merge for a base path, a reference path, and whether the base has an authority.
