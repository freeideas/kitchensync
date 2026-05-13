# 02_url-types: Accepted URL shapes and conversion to file:// URIs

## Behavior

Each URL inside a group is one of three shapes: a bare path (with optional Windows drive letter, forward- or back-slash-delimited), a `file://` URI per RFC 8089, or an `sftp://` URI per RFC 3986. Bare paths are converted to `file://` URIs before population, and relative bare paths are resolved against the caller-supplied `cwd`. Schemes outside `file` and `sftp` are rejected. Derived from SPEC.md section "Grammar" (URL bullet list) and "Output structure" (path conversion).

## $REQ_IDs

- `02.10` — A `file://` URI is accepted and produces `ParsedUrl.scheme = "file"`.
- `02.11` — An `sftp://` URI is accepted and produces `ParsedUrl.scheme = "sftp"`.
- `02.12` — A bare path (e.g. `/abs`, `./relative`) is accepted and produces a `ParsedUrl` with `scheme = "file"`.
- `02.13` — A bare path with a Windows drive letter (e.g. `c:/foo`) is accepted; the resulting `path` includes the drive letter (e.g. `/c:/foo`).
- `02.14` — Backslashes in bare paths are accepted on input and produce a forward-slash-delimited `path` in the output.
- `02.15` — A relative bare path is resolved against the caller-supplied `cwd` so that `ParsedUrl.path` is absolute.
- `02.16` — A URL with any scheme other than `file` or `sftp` is rejected.

## Notes

`cwd` is a forward-slash-delimited absolute directory supplied as an argument; no system calls are made to resolve paths.
