# Gitignore Matcher

A Java 21 library for compiling gitignore-style ignore pattern text into a
deterministic path matcher. It evaluates regular files, directories, symlinks,
and special filesystem entries against ordered ignore rules, hierarchical rule
layers, negation rules, and required built-in exclusions.

The library is for ignore pattern matching only. It does not walk filesystems,
read ignore files, parse command lines or URLs, open network connections, make
sync decisions, copy or rename files, store snapshots, schedule work, or log
diagnostics. Callers provide pattern text and already-known path metadata, then
decide what to list, skip, copy, or delete outside this library.

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

`IgnoreOptions`

| Field | Meaning |
| --- | --- |
| `always_excluded_directory_names` | Directory basenames that are ignored even when a negation pattern matches them. Default: `.kitchensync`. |
| `default_excluded_directory_names` | Directory basenames ignored unless a later negation pattern re-includes them. Default: `.git`. |
| `ignore_symlinks` | When true, symlink entries are always ignored and cannot be re-included. Default: true. |
| `ignore_special_entries` | When true, special entries are always ignored and cannot be re-included. Default: true. |

`PatternLayer`

| Field | Meaning |
| --- | --- |
| `base_path` | Slash-separated directory path where the pattern text applies. Empty string means the root of the matched tree. |
| `pattern_text` | UTF-8 text containing zero or more gitignore-style pattern lines. |
| `source_name` | Optional caller label used only in diagnostics or match explanations. |

`PathEntry`

| Field | Meaning |
| --- | --- |
| `relative_path` | Slash-separated path from the matched tree root, with no leading slash and no trailing slash. |
| `kind` | Entry kind. |

`MatchResult`

| Field | Meaning |
| --- | --- |
| `ignored` | True when the entry should be excluded. |
| `rule_kind` | `always_builtin`, `default_builtin`, `pattern`, or `none`. |
| `negated` | True when the final matching pattern was a negation that re-included the entry. |
| `source_name` | Source label for the final matching pattern, when present. |
| `line_number` | One-based line number for the final matching pattern, when present. |
| `pattern` | Original pattern text for the final matching pattern, when present. |

The matcher may expose less diagnostic detail to ordinary callers, but tests
must be able to observe enough information to verify which rule produced a
match.

### Operations

`IgnoreMatcher.compile(layers, options) -> IgnoreMatcher`

Compiles ordered pattern layers into an immutable matcher. Layers are evaluated
in the supplied order. Within a layer, patterns are evaluated from top to bottom.
Later matching patterns override earlier matching patterns, except for
non-overridable built-ins.

`IgnoreMatcher.empty(options) -> IgnoreMatcher`

Creates an immutable matcher with only built-in exclusions.

`IgnoreMatcher.extend(layer) -> IgnoreMatcher`

Returns a new matcher with `layer` appended after the existing layers. This is
for callers that discover deeper ignore files while traversing a tree.

`IgnoreMatcher.match(entry) -> MatchResult`

Returns whether one path is ignored. Matching is pure and performs no I/O.

`IgnoreMatcher.filter(entries) -> List<PathEntry>`

Returns the input entries that are not ignored, preserving input order.

## Pattern Semantics

Pattern syntax follows gitignore behavior:

- Blank lines are ignored.
- A `#` begins a comment only when it is the first unescaped character on the
  line.
- Leading `!` negates a pattern and re-includes entries ignored by earlier
  overridable rules.
- A literal leading `!` or `#` is written as `\!` or `\#`.
- Trailing spaces are ignored unless escaped with backslash.
- `/` is the path separator in both patterns and input paths.
- A pattern with a trailing `/` matches directories and their descendants only.
- A pattern without `/` matches a basename at any depth below the layer's
  `base_path`.
- A pattern with `/` is relative to the layer's `base_path`. A leading `/`
  anchors the pattern to the layer's `base_path`.
- `*` matches zero or more non-slash characters.
- `?` matches one non-slash character.
- Bracket expressions such as `[abc]`, `[a-z]`, and `[!0-9]` match one
  non-slash character.
- `**/` at the start of a pattern matches zero or more directories.
- `/**` at the end of a pattern matches everything below the preceding path.
- `/**/` in the middle of a pattern matches zero or more directories.
- Other consecutive `*` characters behave like ordinary `*` characters.

When a directory is ignored, its descendants are ignored as well unless the
directory itself is later re-included by a negation pattern before the
descendant is matched. A negation that matches only a descendant path does not
override the still-ignored parent directory. A negation pattern cannot override
entries excluded by
`always_excluded_directory_names`, `ignore_symlinks`, or
`ignore_special_entries`.

The matcher does not give any special meaning to the name of an ignore file.
Callers that need a control file to bypass filtering must avoid filtering that
control file before they parse it.

## Paths

Public operations use normalized relative paths:

- Empty string is valid only as a layer `base_path`, where it means the tree
  root.
- Matched entries must not use an empty path.
- Paths must not start with `/`.
- Paths must not end with `/`.
- Paths must not contain empty segments, `.` segments, `..` segments, backslash,
  or NUL.
- Matching is case-sensitive. The library does not perform filesystem-specific
  case folding.

## Observable Behavior

- Matching is deterministic across operating systems and process runs.
- Compiled matchers are immutable and safe to use concurrently.
- Pattern order is significant; the last matching overridable rule determines
  whether an entry is ignored.
- Built-in symlink and special-entry exclusions apply before pattern negations
  and cannot be overridden.
- Built-in always-excluded directory names apply to that directory and every
  descendant and cannot be overridden.
- Built-in default-excluded directory names behave like initial directory
  patterns and may be overridden by later negation patterns.
- Directory-only patterns do not match regular files with the same basename.
- Public operations do not print to stdout or stderr.

## Error Behavior

Invalid inputs fail with one of these categories and no partial public result:

| Category | Meaning |
| --- | --- |
| `invalid_path` | A layer base path or entry path violates the path rules above. |
| `invalid_pattern_text` | Pattern text contains NUL or cannot be represented as valid text by the host language API. |
| `invalid_options` | A built-in directory name is empty, contains `/`, contains backslash, is `.` or `..`, or contains NUL. |

Malformed bracket expressions are treated as literal text, matching gitignore
behavior, and are not errors.

## Examples

### Basic Patterns

Input:

```text
layers = [
  base_path = ""
  pattern_text =
    *.log
    build/
    !important.log
]

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
match("app.log") = ignored by pattern "*.log"
match("important.log") = not ignored by negation "!important.log"
match("src/build") = ignored by pattern "build/"
match("src/build/out.bin") = ignored because ancestor directory "src/build" is ignored
filter(entries) = [
  regular_file "important.log",
  regular_file "src/main.txt"
]
```

### Hierarchical Layers

Input:

```text
root layer at "":
  *.tmp

docs layer at "docs":
  !keep.tmp
  manual/*.bak

entries = [
  regular_file "scratch.tmp"
  regular_file "docs/draft.tmp"
  regular_file "docs/keep.tmp"
  regular_file "docs/manual/old.bak"
]
```

Output:

```text
match("scratch.tmp") = ignored by root pattern "*.tmp"
match("docs/draft.tmp") = ignored by root pattern "*.tmp"
match("docs/keep.tmp") = not ignored by docs negation "!keep.tmp"
match("docs/manual/old.bak") = ignored by docs pattern "manual/*.bak"
```

### Built-In Exclusions

Input:

```text
options = defaults
layer at "":
  !.git/
  !.kitchensync/
  !link

entries = [
  directory ".git"
  regular_file ".git/config"
  directory ".kitchensync"
  regular_file ".kitchensync/snapshot.db"
  symlink "link"
]
```

Output:

```text
match(".git") = not ignored by negation "!.git/"
match(".git/config") = not ignored
match(".kitchensync") = ignored by always_builtin
match(".kitchensync/snapshot.db") = ignored by always_builtin
match("link") = ignored by always_builtin
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
- Pattern order is honored, including negation re-including entries ignored by
  earlier overridable patterns.
- Layers apply relative to their `base_path`, and later deeper layers can
  override earlier parent layers.
- Directory exclusions apply to descendants, and a descendant-specific negation
  does not bypass an ignored parent directory unless the parent directory is
  re-included first.
- `.kitchensync/` is ignored with default options and cannot be re-included.
- `.git/` is ignored with default options and can be re-included with a
  negation pattern.
- Symlinks and special entries are ignored with default options and cannot be
  re-included.
- Invalid paths, invalid options, and NUL-containing pattern text report the
  specified errors without producing a matcher.
- Compiled matchers are immutable and safe for concurrent calls to `match`.
- No public operation emits stdout or stderr.

Scenarios to avoid:

- Do not test real filesystem traversal, file metadata discovery, symlink target
  resolution, or special-file creation.
- Do not test SFTP, SSH authentication, host-key verification, or connection
  pooling.
- Do not test SQLite schema, path hashing, timestamps, tombstones, or snapshot
  updates.
- Do not test sync conflict decisions, directory listing concurrency, file-copy
  scheduling, BAK/TMP path construction, or transfer pipelines.
- Do not test command-line parsing, URL normalization, fallback URL selection,
  peer roles, startup reachability, or help text.
- Do not test when an ignore file is discovered or read; callers own ignore-file
  resolution order.

## Semantic Anchors

This specification is anchored in:

- Gitignore pattern syntax and precedence as documented by `gitignore(5)`
- The semantic source sections for ignore pattern format, hierarchical ignore
  rules, built-in excludes, symlink and special-file exclusion, and the rule
  that ignore-file resolution is performed before filtering ordinary entries
