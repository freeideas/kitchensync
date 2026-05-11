# Convert between filesystem paths and `file://` URIs per RFC 8089.

## Purpose
Implement the `file` URI scheme: turn a filesystem path into a `file://` URI string and turn a `file://` URI back into a filesystem path. The url-parser glue uses this to coerce bare-path peer arguments (including Windows drive-letter paths like `c:\foo` and relative paths like `./photos`) into `file://` URLs, and to recover absolute filesystem paths from `file://` URLs when normalizing. This component knows nothing about peers, brackets, schemes other than `file`, or any kitchensync concept.

## API surface

### Detection
`is_file_uri(s: string) -> bool` — true if the string begins with the `file:` scheme (case-insensitive). Performs no parsing or validation beyond the scheme check.

`looks_like_bare_path(s: string) -> bool` — true if the string does **not** begin with a recognised URI scheme and so should be treated as a filesystem path. Returns true for POSIX absolute paths (`/foo`), POSIX relative paths (`./foo`, `foo/bar`), Windows DOS-style paths (`c:\foo`, `c:foo`, `C:/foo`), and Windows UNC paths (`\\server\share`).

### Path → URI
`path_to_file_uri(path: string, cwd: string) -> string` — produce the `file://` URI form of a filesystem path per RFC 8089 §2. Rules:
- POSIX absolute paths become `file:///<path>` (empty authority, leading `/` of path preserved).
- POSIX relative paths are resolved against `cwd` (which must itself be absolute) before encoding.
- Windows DOS-style paths produce `file:///<drive>:/<rest>`. Drive-letter paths missing a separator (`c:foo`) are resolved against `cwd`. Backslashes are converted to forward slashes in the URI.
- Windows UNC paths `\\server\share\rest` produce `file://server/share/rest` (the server appears in the authority component).
- The path component is percent-encoded so that the result satisfies the RFC 3986 `path` production. Unreserved characters are left as-is; reserved characters that are not allowed in a path segment are encoded; `/` separators are preserved.

### URI → Path
`file_uri_to_path(uri: string) -> string | FileUriError` — extract the filesystem path from a `file://` URI per RFC 8089 §2. Rules:
- An empty authority and an authority of `localhost` both mean "the local filesystem"; the returned path begins with `/` on POSIX.
- A non-empty, non-`localhost` authority is treated as a Windows UNC server name; the returned path begins with `\\<authority>\` and uses backslashes between the authority and the path on Windows-style output, forward slashes on POSIX-style output (the caller chooses via `style`, see below).
- On a path of the form `/<letter>:/<rest>` or `/<letter>:` (single ASCII letter followed by colon), the leading `/` is dropped and the result is a Windows DOS-style path.
- Percent-encoded octets in the path are decoded.
- Returns a structured `FileUriError` if the input is not a syntactically valid `file:` URI or if its path cannot be interpreted as a local filesystem path.

`file_uri_to_path` may accept an optional `style: "posix" | "windows"` argument that controls the output form (separator and drive-letter formatting). If absent, the host platform's native style is used.

### Errors
`FileUriError` is a structured value carrying a short human-readable message and, where applicable, the offset within the input where the problem was detected. The component does not write to stdout/stderr.

## Anchoring
- `file:` URI scheme, the host-may-be-localhost rule, DOS-drive-letter path handling, UNC path handling, and the path-to-URI / URI-to-path conversion rules: RFC 8089 §2 and Appendix D / E.
- `file://`, URI scheme syntax, authority, path, percent-encoding, unreserved characters: RFC 3986 §2.1, §2.3, §3.2, §3.3.
- "Absolute path", "relative path", "current working directory", path separators (`/` vs `\`), drive letter, UNC: host-operating-system filesystem primitives, as described in RFC 8089.
- `string`, `bool`, optional/absent, and structured-error values: host-language primitives.
