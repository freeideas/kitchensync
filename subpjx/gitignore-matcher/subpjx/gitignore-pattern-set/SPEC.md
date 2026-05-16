# Gitignore Pattern Set

A Java 21 library for compiling one block of gitignore-style pattern text into
an immutable ordered pattern set, then evaluating normalized relative paths
against that set. It is for the syntax and precedence of gitignore pattern
lines only.

The library does not walk filesystems, read ignore files, manage hierarchical
ignore-file layers, apply built-in exclusions, decide whether an ignored parent
directory prevents a descendant from being visible, resolve symlink targets,
inspect special files, filter collections, parse command lines or URLs, open
network connections, store snapshots, or log diagnostics.

## Public API

The API may use normal Java classes, records, interfaces, or equivalent types,
but it must expose this behavior.

### Data Shapes

`EntryKind`

| Value | Meaning |
| --- | --- |
| `regular_file` | A normal file. |
| `directory` | A directory. |
| `symlink` | A symbolic link, whether it points to a file or directory. |
| `special` | A device, FIFO, socket, or any other non-regular, non-directory entry. |

`PatternSetSource`

| Field | Meaning |
| --- | --- |
| `pattern_text` | Text containing zero or more gitignore-style pattern lines. |
| `source_name` | Optional caller label copied into match results. |

`PathEntry`

| Field | Meaning |
| --- | --- |
| `relative_path` | Slash-separated path relative to the pattern set root, with no leading slash and no trailing slash. |
| `kind` | Entry kind. |

`PatternDecision`

| Value | Meaning |
| --- | --- |
| `ignore` | The final matching pattern excludes the entry. |
| `include` | The final matching pattern is a negation that includes the entry. |
| `none` | No pattern matches the entry. |

`PatternMatch`

| Field | Meaning |
| --- | --- |
| `decision` | `ignore`, `include`, or `none`. |
| `negated` | True when `decision` is `include`. |
| `source_name` | Source label for the final matching pattern, when present. |
| `line_number` | One-based line number for the final matching pattern, when present. |
| `pattern` | Original effective pattern line for the final matching pattern, when present. |

For `decision = none`, `negated` is false and pattern metadata is absent.

### Operations

`GitignorePatternSet.compile(source) -> GitignorePatternSet`

Compiles a pattern text block into an immutable ordered pattern set. Patterns
are evaluated from top to bottom, and the last matching pattern determines the
decision for a path.

`GitignorePatternSet.empty() -> GitignorePatternSet`

Creates an immutable pattern set with no patterns. Every valid path returns
`decision = none`.

`GitignorePatternSet.match(entry) -> PatternMatch`

Returns the final pattern decision for one path. Matching is pure and performs
no I/O.

## Pattern Text Parsing

- Blank lines are ignored.
- A `#` begins a comment only when it is the first unescaped character on the
  line.
- Leading `!` negates a pattern and produces an `include` decision when it is
  the final match.
- A literal leading `!` or `#` is written as `\!` or `\#`.
- Trailing spaces are ignored unless escaped with backslash.
- Line terminators are not part of a pattern.
- Patterns are evaluated in source order after ignored lines are removed.
- `line_number` reports the original one-based line number in `pattern_text`.

## Pattern Matching

- `/` is the path separator in both patterns and input paths.
- A pattern with a trailing `/` matches directories and their descendants only.
- A trailing slash pattern does not match a non-directory entry with the same
  path.
- A pattern without `/` matches a basename at any depth below the pattern set
  root.
- A pattern with `/` is relative to the pattern set root.
- A leading `/` anchors the pattern to the pattern set root.
- `*` matches zero or more non-slash characters.
- `?` matches one non-slash character.
- Bracket expressions such as `[abc]`, `[a-z]`, and `[!0-9]` match one
  non-slash character.
- `**/` at the start of a pattern matches zero or more directories.
- `/**` at the end of a pattern matches everything below the preceding path.
- `/**/` in the middle of a pattern matches zero or more directories.
- Other consecutive `*` characters behave like ordinary `*` characters.

A directory-only pattern can match a descendant path even when the descendant
entry is not itself a directory. For example, `build/` matches `build`,
`build/out.bin`, and `src/build/out.bin`, but it does not match a regular file
whose complete path is `build`.

This library reports the final syntactic pattern decision for the supplied
path. It does not decide whether an ignored ancestor directory should block a
later descendant include decision.

## Paths

Public operations use normalized relative paths:

- Matched entries must not use an empty path.
- Paths must not start with `/`.
- Paths must not end with `/`.
- Paths must not contain empty segments, `.` segments, `..` segments,
  backslash, or NUL.
- Matching is case-sensitive.
- The library does not perform filesystem-specific case folding.

## Observable Behavior

- Matching is deterministic across operating systems and process runs.
- Compiled pattern sets are immutable and safe to use concurrently.
- Pattern order is significant; the last matching pattern determines the
  decision.
- Directory-only patterns use `EntryKind.directory` only for the entry at the
  matched directory path. Descendant paths may have any entry kind.
- Symlink and special entries have no built-in behavior. They are matched by
  path syntax like ordinary non-directory entries unless a directory-only
  pattern matches one of their ancestor directories.
- Public operations do not print to stdout or stderr.

## Error Behavior

Invalid inputs fail with one of these categories and no partial public result:

| Category | Meaning |
| --- | --- |
| `invalid_path` | An entry path violates the path rules above. |
| `invalid_pattern_text` | Pattern text contains NUL or cannot be represented as valid text by the host language API. |

Malformed bracket expressions are treated as literal text, matching gitignore
behavior, and are not errors.

## Examples

### Basic Patterns

Input:

```text
pattern_text =
  *.log
  build/
  !important.log

entries = [
  regular_file "app.log"
  regular_file "important.log"
  directory "src/build"
  regular_file "src/build/out.bin"
  regular_file "src/main.txt"
]
```

Output:

```text
match("app.log") = ignore by pattern "*.log"
match("important.log") = include by pattern "!important.log"
match("src/build") = ignore by pattern "build/"
match("src/build/out.bin") = ignore by pattern "build/"
match("src/main.txt") = none
```

### Anchoring And Double Star

Input:

```text
pattern_text =
  /docs/*.md
  **/tmp/**

entries = [
  regular_file "docs/readme.md"
  regular_file "src/docs/readme.md"
  regular_file "tmp/cache.bin"
  regular_file "src/tmp/cache.bin"
]
```

Output:

```text
match("docs/readme.md") = ignore by pattern "/docs/*.md"
match("src/docs/readme.md") = none
match("tmp/cache.bin") = ignore by pattern "**/tmp/**"
match("src/tmp/cache.bin") = ignore by pattern "**/tmp/**"
```

### Escaped Pattern Characters

Input:

```text
pattern_text =
  # comment
  \#literal
  \!literal
  name\ 

entries = [
  regular_file "#literal"
  regular_file "!literal"
  regular_file "name "
  regular_file "name"
]
```

Output:

```text
match("#literal") = ignore by pattern "\#literal"
match("!literal") = ignore by pattern "\!literal"
match("name ") = ignore by pattern "name\ "
match("name") = none
```

## Testing Requirements

Tests are black-box tests of the public API. No external service account, SFTP
server, SSH key, SQLite database, local filesystem tree fixture, or network
access is required.

Required scenarios:

- Blank lines, comments, escaped leading `#` and `!`, and escaped trailing
  spaces are parsed correctly.
- `*`, `?`, bracket expressions, leading slash anchoring, interior slashes,
  trailing slash directory patterns, and the supported `**` forms match the
  specified paths.
- Pattern order is honored, including negation overriding earlier matching
  patterns.
- Directory-only patterns match directories and descendants, but do not match
  a non-directory entry at the same complete path.
- Symlink and special entries have no built-in exclusion behavior.
- Invalid paths and NUL-containing pattern text report the specified errors
  without producing a pattern set or match result.
- Malformed bracket expressions are matched as literal text.
- Compiled pattern sets are immutable and safe for concurrent calls to
  `match`.
- No public operation emits stdout or stderr.

Scenarios to avoid:

- Do not test hierarchical ignore layers, layer base paths, built-in excludes,
  or ignored-ancestor re-inclusion policy.
- Do not test real filesystem traversal, file metadata discovery, symlink
  target resolution, or special-file creation.
- Do not test SFTP, SSH authentication, host-key verification, or connection
  pooling.
- Do not test SQLite schema, path hashing, timestamps, tombstones, or snapshot
  updates.
- Do not test sync conflict decisions, directory listing concurrency,
  file-copy scheduling, backup path construction, temporary path construction,
  or transfer pipelines.
- Do not test command-line parsing, URL normalization, fallback URL selection,
  peer roles, startup reachability, or help text.
- Do not test when an ignore file is discovered or read.

## Semantic Anchors

This specification is anchored in `gitignore(5)` pattern syntax and precedence.
