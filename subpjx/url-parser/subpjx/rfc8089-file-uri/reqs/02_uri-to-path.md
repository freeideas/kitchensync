# 02_uri-to-path: Convert `file://` URIs back to filesystem paths

## Behavior
`file_uri_to_path(uri, style?)` extracts the filesystem path from a `file://` URI per RFC 8089 §2. The authority field selects between local-filesystem and Windows UNC interpretations, paths whose first segment is a single ASCII letter followed by a colon are treated as Windows DOS-style drive paths, and percent-encoded octets are decoded. The optional `style` argument controls whether the output is formatted in POSIX or Windows form; absent, the host platform's native style is used. Derived from `./specs/SPEC.md` → "API surface" → URI → Path.

## $REQ_IDs
- `02.18` — An empty authority means the local filesystem; the returned path begins with `/` on POSIX.
- `02.19` — An authority of `localhost` means the local filesystem; the returned path begins with `/` on POSIX.
- `02.20` — A non-empty, non-`localhost` authority is treated as a Windows UNC server name; the returned path begins with `\\<authority>\`.
- `02.21` — A path of the form `/<letter>:/<rest>` (single ASCII letter followed by a colon) has its leading `/` dropped and is returned as a Windows DOS-style path.
- `02.22` — A path of the form `/<letter>:` (drive letter alone) has its leading `/` dropped and is returned as a Windows DOS-style path.
- `02.23` — Percent-encoded octets in the URI path are decoded in the returned filesystem path.
- `02.24` — When `style="posix"` is supplied, the output uses POSIX form (forward-slash separators).
- `02.25` — When `style="windows"` is supplied, the output uses Windows form (backslash separators and DOS drive-letter formatting).
- `02.26` — When `style` is absent, the output uses the host platform's native style.
