# 02_parse-uri: parse_uri accepts URI strings and reports errors

## Behavior

`parse_uri` is the library's entry point for turning a URI string into a structured `Uri` value. It accepts both full URIs (with a scheme) and relative references, and yields a `UriParseError` for malformed input. The error is a structured value — never a stdout/stderr write. Derived from `specs/SPEC.md` "API surface — Parsing" (RFC 3986 §3, §4.1).

## $REQ_IDs

- `02.1` — `parse_uri` accepts a URI string that has a scheme and returns its components.
- `02.2` — `parse_uri` accepts a relative reference (no scheme) and returns its components.
- `02.3` — `parse_uri` returns a `UriParseError` for malformed input.
- `02.4` — `UriParseError` carries a short human-readable message and the offset within the input where the error was detected.
- `02.5` — `parse_uri` does not write to stdout or stderr.
