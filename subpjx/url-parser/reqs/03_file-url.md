# 03_file-url: file:// URL parsing and path resolution

## Behavior

Bare filesystem paths (with no URL scheme) are treated as `file://` URLs. This includes Unix-style absolute paths, drive-letter paths like `c:\foo`, and relative paths like `./relative`. Relative paths are resolved against the process's current working directory at parse time. When `normalize_url` is asked for the canonical identity of a `file://` URL, it resolves the path to an absolute filesystem path. Derived from `SPEC.md` §"Peer-argument parsing" (bare-path-to-`file://` rule) and §"URL normalization" (`file://` absolute-path rule).

## $REQ_IDs
- `03.1` — A bare filesystem path argument with no URL scheme is parsed as a `file://` URL.
- `03.2` — A Windows-style drive-letter path such as `c:\foo` is parsed as a `file://` URL.
- `03.3` — A relative path argument like `./relative` is parsed as a `file://` URL whose path is resolved against the process's current working directory at parse time.
- `03.4` — Normalizing a `file://` URL resolves its path to an absolute filesystem path in the returned canonical identity string.
