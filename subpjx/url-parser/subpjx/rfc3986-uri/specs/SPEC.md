# Parse, serialize, and syntax-normalize URIs per RFC 3986.

## Purpose
Provide a generic URI engine that turns a URI string into its component parts, turns components back into a string, and applies RFC 3986 §6 syntax-based normalization. The url-parser glue uses this once per URL it sees — to split each peer URL into scheme/authority/path/query/fragment, to percent-decode unreserved characters, and to normalize case and path before applying its own kitchensync-specific transforms (bracket grammar, +/- prefixes, mc/ct/ka query parameters, default sftp user, bare-path coercion). This component knows nothing about peers, brackets, schemes' default ports, file vs sftp, or any kitchensync concept.

## API surface

### Parsing
`parse_uri(s: string) -> Uri | UriParseError` — parse a URI string into its components per RFC 3986 §3. Accepts both URIs (with a scheme) and relative references. The returned `Uri` exposes:
- `scheme`: the scheme component if present, else absent. Lowercased on access is the caller's choice; the raw form is preserved.
- `authority`: if present, an `Authority` record with `userinfo` (optional), `host`, and `port` (optional integer). `userinfo` may itself be split into `user` and `password` at the first `:`. An empty userinfo (the `@` is present but nothing precedes it) is reported as present-but-empty, distinct from absent.
- `path`: the path component as a string. Always present; may be empty.
- `query`: the query component if present, else absent. The component does not split on `&`/`=` — that is a higher-level concern.
- `fragment`: the fragment component if present, else absent.

`UriParseError` is a structured value carrying a short human-readable message and the offset within the input where the error was detected. The component does not write to stdout/stderr.

### Serialization
`serialize_uri(u: Uri) -> string` — render a `Uri` back to its RFC 3986 §5.3 string form. The output is `parse_uri`-round-trippable for any well-formed input.

### Percent-encoding
`percent_encode(s: string, safe: CharClass) -> string` — encode `s` per RFC 3986 §2.1, leaving characters in the named `CharClass` unencoded. Provided classes match RFC 3986: `unreserved`, `gen_delims`, `sub_delims`, `reserved` (= `gen_delims ∪ sub_delims`), and the per-component "allowed" sets implied by §3 (`scheme`, `userinfo`, `host`, `path`, `query`, `fragment`).

`percent_decode(s: string) -> string | PercentDecodeError` — decode all `%HH` triplets in `s` per RFC 3986 §2.1. Invalid triplets (`%` not followed by two hex digits) yield a structured error.

`percent_decode_unreserved(s: string) -> string | PercentDecodeError` — decode only those `%HH` triplets whose decoded byte is in the `unreserved` class (RFC 3986 §6.2.2.2). Other valid triplets are uppercased (`%2f` → `%2F`) and left in place. Invalid triplets yield a structured error.

### Normalization
`normalize_uri(u: Uri) -> Uri` — apply RFC 3986 §6.2.2 syntax-based normalization to a parsed `Uri`:
- Lowercase the scheme (§6.2.2.1).
- Lowercase the host (§6.2.2.1).
- Decode unreserved percent-encoded characters in userinfo, host (reg-name), path, query, and fragment (§6.2.2.2).
- Uppercase the hex digits of remaining percent-encoded triplets (§6.2.2.1).
- Apply the `remove_dot_segments` algorithm (§5.2.4) to the path (§6.2.2.3).

This component does **not** apply scheme-based normalization (§6.2.3) — it does not know any scheme's default port, nor that an empty path under an authority should become `/`. Such rules are the caller's responsibility.

### Path utilities
`remove_dot_segments(path: string) -> string` — the RFC 3986 §5.2.4 algorithm in isolation. Useful to callers that want to normalize a path without round-tripping through `Uri`.

`merge_paths(base_path: string, ref_path: string, base_has_authority: bool) -> string` — the RFC 3986 §5.2.3 path merge used in reference resolution. (Provided for completeness; callers needing reference resolution can compose this with `remove_dot_segments`.)

### Character classification
`is_unreserved(c: char) -> bool`, `is_reserved(c: char) -> bool`, `is_gen_delim(c: char) -> bool`, `is_sub_delim(c: char) -> bool` — the RFC 3986 §2.2/§2.3 character-class predicates, exposed for callers that want to validate or classify input without parsing.

## Anchoring
- `Uri`, `scheme`, `authority`, `userinfo`, `host`, `port`, `path`, `query`, `fragment`, the syntax of each component, and the parsing/serialization grammar: RFC 3986 §3 and §5.3.
- `parse_uri` accepting both URIs and relative references: RFC 3986 §4.1.
- `percent_encode`, `percent_decode`, `%HH` triplet syntax: RFC 3986 §2.1.
- `unreserved`, `reserved`, `gen-delims`, `sub-delims` character classes: RFC 3986 §2.2 and §2.3.
- `normalize_uri` and the specific normalization rules (case, percent-encoding, dot-segment removal): RFC 3986 §6.2.2.
- `remove_dot_segments`: RFC 3986 §5.2.4.
- `merge_paths`: RFC 3986 §5.2.3.
- `string`, `char`, `bool`, integer, optional/absent, and structured-error values: host-language primitives.
