# 02_detection: Detect `file:` URIs and bare filesystem paths

## Behavior
The library exposes two predicate functions used by callers to classify an input string before deciding how to process it: `is_file_uri` checks whether a string is in the `file:` URI scheme, and `looks_like_bare_path` checks whether a string lacks any recognised URI scheme and so should be treated as a filesystem path. Neither performs full URI parsing or path validation. Derived from `./specs/SPEC.md` → "API surface" → Detection.

## $REQ_IDs
- `02.1` — `is_file_uri` returns true for strings that begin with the `file:` scheme.
- `02.2` — `is_file_uri` matches the scheme case-insensitively (e.g. `FILE:`, `File:` are accepted).
- `02.3` — `is_file_uri` returns false for strings that do not begin with the `file:` scheme.
- `02.4` — `looks_like_bare_path` returns true for POSIX absolute paths such as `/foo`.
- `02.5` — `looks_like_bare_path` returns true for POSIX relative paths such as `./foo` and `foo/bar`.
- `02.6` — `looks_like_bare_path` returns true for Windows DOS-style paths such as `c:\foo`, `c:foo`, and `C:/foo`.
- `02.7` — `looks_like_bare_path` returns true for Windows UNC paths such as `\\server\share`.
- `02.8` — `looks_like_bare_path` returns false for strings that begin with a recognised URI scheme.
