# Compile gitignore-style pattern text into structured, matchable rules

## Purpose
Parse the textual content of a single `.syncignore`/`.gitignore` file according to the gitignore pattern grammar and produce a structured `PatternSet` — an ordered list of compiled patterns, each labelled with the syntactic flags it declared (negation, anchoring, directory-only) and bearing a predicate that decides whether a candidate relative path matches the pattern's body. This component is the parsing-and-compilation half of gitignore matching: it converts surface syntax into a regular, matchable form. It does no filesystem I/O and is unaware of directory scopes, hierarchy stacking, or built-in always-ignored entries — those concerns belong to the host that consumes a `PatternSet`. This implements the "Pattern Format" portion of gitignore semantics as documented in the gitignore syntax reference.

## API surface

### Compiling

`compile_patterns(text: string) -> (PatternSet, Diagnostics)`

Takes the raw text of one gitignore-format file and returns a `PatternSet` together with a `Diagnostics` list. Parsing proceeds line by line:

- Blank lines and lines whose first non-whitespace character is `#` are skipped.
- A literal `#` at the start of a pattern can be escaped as `\#`.
- Trailing whitespace on a line is stripped unless the final whitespace character is escaped with backslash.
- A leading `!` sets the pattern's negation flag and is consumed.
- A leading `/` sets the pattern's anchor flag and is consumed.
- A trailing `/` sets the pattern's directory-only flag and is consumed.
- Backslash escapes a following metacharacter so it matches literally.

Within the pattern body the following wildcards are recognised:
- `*` matches a run (possibly empty) of any characters other than `/`.
- `?` matches exactly one character other than `/`.
- `[abc]` is a character class; `[!abc]` / `[^abc]` is a negated class; ranges `a-z` are supported; a literal `]` is allowed as the first character of the class.
- `**` is recognised as a directory-spanning wildcard in the three documented positions: leading `**/` (the pattern matches at any depth), trailing `/**` (the pattern matches anything inside the matched directory), and `/**/` (zero or more intermediate directory segments). `**` adjacent to other characters in a path segment falls back to single-`*` semantics.

A pattern that contains a `/` anywhere except as the trailing directory marker is treated as containing path separators and is matched against the full candidate relative path; a pattern with no internal `/` is matched against any single path segment within the candidate path. (This mirrors gitignore's rule that a slash other than at the end constrains the pattern to a path-shaped match.)

If a line cannot be compiled (for example, an unclosed character class, or `\` at end of line with nothing to escape) the offending line is skipped and an entry is appended to `Diagnostics` describing the line number and the reason. Compilation as a whole always succeeds; `PatternSet` always exists.

### PatternSet shape

A `PatternSet` is an ordered sequence of `CompiledPattern` values. Order in the sequence matches order in the source text. Each `CompiledPattern` exposes:

- `is_negation: bool` — whether the source pattern began with `!`.
- `is_anchored: bool` — whether the source pattern began with `/` (i.e. it is anchored to "the directory the file lives in" rather than matching at any depth).
- `is_dir_only: bool` — whether the source pattern ended with `/` (i.e. it matches directories only).
- `matches(path: relative-path) -> bool` — predicate that decides, given a forward-slash-separated relative path with no leading slash, whether the path's text matches the pattern body. The predicate ignores anchoring and directory-only concerns; it answers a pure textual question about the body. For an anchored pattern the caller supplies a path already made relative to the pattern's anchor directory; for an unanchored pattern with no internal `/`, the caller may ask whether any single path segment matches by passing that segment.
- `source: string` — the original source line (post-strip, pre-flag-consumption) for diagnostics and round-trip display.

`Diagnostics` is an ordered sequence of `{line_number: int, line_text: string, reason: string}` records, one per skipped line.

### Operations on PatternSet

`empty_pattern_set() -> PatternSet` — a `PatternSet` containing no patterns. Useful for callers that want to represent "no rules from this file" without re-parsing an empty string.

`pattern_count(set: PatternSet) -> int` — the number of compiled patterns in the set.

`pattern_at(set: PatternSet, index: int) -> CompiledPattern` — random access by index; indices are zero-based and follow source order.

No mutation operations are exposed; a `PatternSet` is immutable once compiled.

## Anchoring
- `compile_patterns`, the pattern grammar (blank/comment lines, backslash escape, leading `!` for negation, leading `/` for anchoring, trailing `/` for directory-only, `*`, `?`, `[…]`, `**` in its three documented positions, the slash-anywhere-else-makes-it-path-shaped rule): the gitignore pattern syntax reference at git-scm.com, the recognised external standard for this format.
- `CompiledPattern.is_negation`, `is_anchored`, `is_dir_only`: the three syntactic flags named explicitly in the gitignore syntax reference.
- `CompiledPattern.matches(path)`: a pure textual predicate over a host-language string; the path shape (forward-slash-separated, no leading slash, relative) is the standard POSIX relative-path convention.
- `Diagnostics`: a list of host-language records describing skipped lines.
- `PatternSet`, `empty_pattern_set`, `pattern_count`, `pattern_at`: a standard immutable ordered-sequence abstraction (a host-language primitive container), parameterised over `CompiledPattern`.
