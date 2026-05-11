# 03_errors: Structured `FileUriError` for invalid input

## Behavior
When `file_uri_to_path` is given input that is not a syntactically valid `file:` URI, or whose path cannot be interpreted as a local filesystem path, it returns a structured `FileUriError` value carrying a short human-readable message and, where applicable, the offset within the input where the problem was detected. The component reports problems exclusively through return values and never writes to stdout or stderr. Derived from `./specs/SPEC.md` → "API surface" → URI → Path and Errors.

## $REQ_IDs
- `03.1` — `file_uri_to_path` returns a `FileUriError` when the input is not a syntactically valid `file:` URI.
- `03.2` — `file_uri_to_path` returns a `FileUriError` when the URI's path cannot be interpreted as a local filesystem path.
- `03.3` — `FileUriError` carries a human-readable message.
- `03.4` — `FileUriError` carries the offset within the input where the problem was detected, where applicable.
- `03.5` — The component does not write to stdout or stderr.
