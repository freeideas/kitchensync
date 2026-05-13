# Gitignore-syntax path matcher

## Purpose
Compile gitignore-syntax pattern text into a reusable matcher that decides whether a relative path is ignored. Pure functions over text and paths; no filesystem access, no I/O, no concurrency. Supports hierarchical composition: patterns declared at a deeper directory level add to, and may override via negation, patterns declared at shallower levels — exactly as gitignore specifies.

## API surface

### Compile
- `compile(text)` → `Patterns` — parse a chunk of gitignore-syntax pattern text (the verbatim contents of one ignore file) into an opaque pattern set. Blank lines and lines beginning with `#` are skipped. Every other line is a single pattern. Order within `text` is preserved: within a single compiled set, later patterns override earlier ones for any path that both would match.

### Match
- `match(stack, relative_path, is_directory)` → `Ignored` | `NotIgnored`
  - `stack` is an ordered sequence of `(scope, Patterns)` pairs, shallowest first. Each `scope` is the directory at which the corresponding `Patterns` was declared, expressed as a forward-slash-delimited relative path from a caller-chosen root (the empty string denotes the root itself).
  - `relative_path` is the candidate path, expressed relative to the same root, forward-slash-delimited, no leading or trailing slash, no `.` or `..` components. The caller is responsible for normalization.
  - `is_directory` is true when the candidate is a directory and false when it is a file. Patterns whose textual form ends with `/` match directories only.
  - A pattern declared at scope `D` applies only to candidates strictly inside `D` — that is, candidates whose path components begin with the components of `D`. Before matching such a pattern, the components of `D` are stripped from the candidate's path.
  - Patterns from later stack entries are evaluated after those from earlier stack entries. Within one stack entry, later patterns override earlier ones. The result is `Ignored` iff the most-recently-applied matching pattern is positive; if no pattern matches, or the most-recently-applied matching pattern is a negation, the result is `NotIgnored`.
  - Negation cannot re-include a path whose parent directory is itself excluded by an unnegated pattern. When evaluating a path, if any strict ancestor directory of the path would be classified `Ignored` (treating it as a directory and considering only patterns whose scope is an ancestor of, or equal to, the ancestor being tested), the candidate is `Ignored` regardless of any negation that would otherwise apply.

### Empty input
- `compile("")` returns a `Patterns` value that matches nothing.
- `match` invoked with an empty `stack` returns `NotIgnored` for every input.

## Pattern syntax

Patterns follow the gitignore pattern format documented at https://git-scm.com/docs/gitignore. Summary:

- Blank lines match nothing. A line beginning with `#` is a comment; to use a literal `#` or `!` at the start of a pattern, escape it with `\`.
- Trailing whitespace is stripped unless escaped with `\`.
- A leading `!` negates the pattern; a matching path previously classified as ignored becomes not-ignored (subject to the parent-directory restriction above).
- A trailing `/` restricts the pattern to directories.
- A pattern containing no `/`, or only a trailing one, matches at any depth below the scope at which it was declared.
- A pattern containing a `/` anywhere other than at the very end is anchored at the scope at which it was declared.
- Glob tokens: `*` matches any run of non-`/` characters; `?` matches one non-`/` character; `[abc]`, `[a-z]`, `[!abc]` match a character class; `**` matches zero or more path components (with the usual gitignore restrictions: a leading `**/` matches any depth, a trailing `/**` matches everything inside, and `a/**/b` matches `b` zero or more directories below `a`).
- A leading `/` anchors at the declaring scope's root.

## Anchoring

- Pattern syntax (`*`, `?`, `[...]`, `**`, `/`, `!`, `#`, trailing `/`, leading `/`, negation semantics, parent-directory restriction): the gitignore pattern format at https://git-scm.com/docs/gitignore.
- "Relative path", "directory", "path component": host-language string and filesystem-path primitives. Forward-slash-delimited; no `.` or `..`; no leading or trailing slash.
- "Pattern set", "match", "compile": pure functions; no external dependency beyond the host language's standard library.
