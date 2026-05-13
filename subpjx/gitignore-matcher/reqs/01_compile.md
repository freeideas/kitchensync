# 01_compile: parsing gitignore pattern text into a reusable pattern set

## Behavior
`compile` accepts the verbatim contents of one ignore file (gitignore-syntax pattern text) and produces a reusable pattern set that `match` later consults. Compilation defines how the input text is partitioned into patterns: blank lines and `#`-comment lines are discarded; every other line becomes one pattern; the input order is preserved so that later patterns can override earlier ones within the same set. The escape conventions for literal `#`/`!` line starts and for trailing whitespace are also defined here. Derived from `./specs/SPEC.md` sections "API surface › Compile", "Empty input", and "Pattern syntax".

## $REQ_IDs
- `01.1` — Blank lines in the input do not classify any path as `Ignored`.
- `01.2` — Lines whose first character is `#` are treated as comments and do not classify any path as `Ignored`.
- `01.3` — Within one compiled pattern set, when two patterns would both match the same path, the pattern that appeared later in the input is the one whose verdict applies.
- `01.4` — `compile("")` produces a pattern set that does not classify any path as `Ignored`.
- `01.5` — Trailing whitespace on a pattern line is stripped unless the final whitespace character is escaped with `\`.
- `01.6` — A pattern whose intended first character is `#` is written as `\#` so the line is treated as a pattern, not as a comment.
- `01.7` — A pattern whose intended first character is `!` is written as `\!` so the leading `!` is part of the pattern rather than the negation marker.

## Notes
The pattern set returned by `compile` is opaque to callers — only its observable effect when handed to `match` is testable. The "later overrides earlier" rule is observed by compiling a positive pattern followed by a negation (or vice versa) that both match the same path and checking the verdict.
