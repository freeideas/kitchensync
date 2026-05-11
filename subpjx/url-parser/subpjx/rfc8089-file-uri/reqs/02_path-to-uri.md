# 02_path-to-uri: Convert filesystem paths to `file://` URIs

## Behavior
`path_to_file_uri(path, cwd)` produces the `file://` URI form of a filesystem path per RFC 8089 §2. It handles POSIX absolute and relative paths, Windows DOS-style paths (with or without a separator after the drive letter, with backslashes or forward slashes), and Windows UNC paths. The path component of the resulting URI is percent-encoded so it satisfies the RFC 3986 path production. Derived from `./specs/SPEC.md` → "API surface" → Path → URI.

## $REQ_IDs
- `02.9` — A POSIX absolute path produces `file:///<path>` with empty authority and the leading `/` of the path preserved.
- `02.10` — A POSIX relative path is resolved against `cwd` before encoding.
- `02.11` — A Windows DOS-style path produces `file:///<drive>:/<rest>`.
- `02.12` — Backslashes in a Windows path are converted to forward slashes in the URI.
- `02.13` — A drive-letter path missing a separator (e.g. `c:foo`) is resolved against `cwd`.
- `02.14` — A Windows UNC path `\\server\share\rest` produces `file://server/share/rest` with `server` in the authority component.
- `02.15` — Unreserved characters in the path are not percent-encoded.
- `02.16` — Reserved characters that are not allowed in a path segment are percent-encoded.
- `02.17` — `/` separators in the path are preserved (not percent-encoded).
