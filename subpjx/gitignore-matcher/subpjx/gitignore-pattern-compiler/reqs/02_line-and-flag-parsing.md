# 02_line-and-flag-parsing: Line preprocessing and syntactic flag consumption

## Behavior
`compile_patterns` walks the input text line by line. Some lines never become patterns (blank or pure comments); others have leading/trailing markers that are stripped from the body and exposed as boolean flags on the resulting `CompiledPattern`. The marker characters are `!` for negation (leading), `/` for anchoring (leading), and `/` for directory-only (trailing). Trailing whitespace is stripped unless a backslash protects the final whitespace character, and `\#` allows a pattern to literally start with `#`. Derived from `specs/SPEC.md` section "Compiling".

## $REQ_IDs
- `02.1` — A blank line produces no compiled pattern.
- `02.2` — A line whose first non-whitespace character is `#` produces no compiled pattern.
- `02.3` — A line starting with `\#` produces a pattern whose body begins with a literal `#`.
- `02.4` — Unescaped trailing whitespace on a line is stripped before parsing the body.
- `02.5` — A trailing whitespace character preceded by `\` is retained as part of the body.
- `02.6` — A leading `!` sets `is_negation` to true and is removed from the body before matching.
- `02.7` — A leading `/` sets `is_anchored` to true and is removed from the body before matching.
- `02.8` — A trailing `/` sets `is_dir_only` to true and is removed from the body before matching.
- `02.9` — A pattern without leading `!`, without leading `/`, and without trailing `/` has `is_negation`, `is_anchored`, and `is_dir_only` all false.
