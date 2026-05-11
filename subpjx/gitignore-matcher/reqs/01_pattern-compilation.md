# 01_pattern-compilation: compile_patterns parses raw .syncignore text into a PatternSet

## Behavior
`compile_patterns(text)` takes the raw text content of a single `.syncignore` file and returns a structured `PatternSet`, parsed line-by-line. Compilation is forgiving: blank lines and comments contribute nothing, trailing whitespace is normalized, and individual malformed lines are skipped rather than failing the whole file. Derived from `SPEC.md` §"Compiling pattern text".

## $REQ_IDs
- `01.1` — Blank lines in pattern text contribute no pattern to the resulting PatternSet.
- `01.2` — Lines beginning with `#` are treated as comments and contribute no pattern.
- `01.3` — Unescaped trailing whitespace on a pattern line is stripped before the line is interpreted.
- `01.4` — Trailing whitespace escaped with a backslash is preserved as part of the pattern.
- `01.5` — A malformed pattern line (for example an unclosed character class) produces no pattern in the resulting PatternSet.
- `01.6` — When one line of a pattern file is malformed, the remaining lines in the same file still compile into the PatternSet.
- `01.7` — Compilation returns a diagnostics list alongside the PatternSet identifying any skipped malformed lines.

## Notes
The PatternSet itself becomes observable only after it is pushed into a Matcher and queried via `is_ignored`; tests therefore exercise compilation through the full compile → push_scope → is_ignored chain.
