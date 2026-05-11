# 04_diagnostics: Error tolerance and Diagnostics records

## Behavior
`compile_patterns` never fails as a whole. When a single line cannot be compiled — for example, an unclosed character class or a backslash with nothing to escape at end of line — the offending line is skipped and a diagnostic record is appended to the `Diagnostics` list naming the line number, the original line text, and a reason. The `PatternSet` is always returned, and lines that do compile are included regardless of the presence of malformed neighbours. Derived from `specs/SPEC.md` sections "Compiling" (the error paragraph) and "PatternSet shape" (the `Diagnostics` record description).

## $REQ_IDs
- `04.1` — A line with an unclosed character class is omitted from the `PatternSet` and adds an entry to `Diagnostics`.
- `04.2` — A line ending with a backslash that has nothing to escape is omitted from the `PatternSet` and adds an entry to `Diagnostics`.
- `04.3` — Each `Diagnostics` entry exposes `line_number`, `line_text`, and `reason` fields describing the skipped line.
- `04.4` — `compile_patterns` returns a `PatternSet` (rather than raising) even when the input contains lines that fail to compile.
- `04.5` — Valid lines appearing before or after a skipped line still produce compiled patterns in the returned `PatternSet`.
