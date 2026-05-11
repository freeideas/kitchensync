# 02_wildcards: Single-segment wildcards and character classes

## Behavior
Within a pattern body, the standard gitignore wildcards are recognised: `*` and `?` match runs/characters that do not cross `/`, `[…]` is a character class with optional negation and range syntax, and a backslash escape lets any metacharacter match literally. Derived from `specs/SPEC.md` section "Compiling" — wildcard list.

## $REQ_IDs
- `02.1` — `*` in a pattern body matches a possibly-empty run of any characters other than `/`.
- `02.2` — `?` in a pattern body matches exactly one character other than `/`.
- `02.3` — `[abc]` matches exactly one character from the listed set.
- `02.4` — `[!abc]` and `[^abc]` each match exactly one character that is not in the listed set.
- `02.5` — Ranges of the form `[a-z]` inside a class match any character within the inclusive range.
- `02.6` — A literal `]` as the first character of a class (e.g. `[]abc]`) is treated as part of the class rather than closing it.
- `02.7` — A backslash before a metacharacter causes that character to match literally in the body.
