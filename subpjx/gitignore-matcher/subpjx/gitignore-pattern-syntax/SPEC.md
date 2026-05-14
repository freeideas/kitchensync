# Gitignore Pattern Syntax

## Purpose
Compile gitignore-style pattern lines into pattern rules and evaluate those rules against relative path strings.

## Public API
Data shapes:

- `PatternLine`: one gitignore-style pattern text line
- `PatternRule`: compiled pattern rule
- `PatternMatchInput`: relative `path`, `is_directory`
- `PatternMatchResult`: `matches`, `ignored` or `included`

Operations:

- `compile_patterns(pattern_lines) -> PatternRule[]`
- `match_patterns(pattern_rules, input) -> PatternMatchResult`

## Behavior
Pattern lines use `.gitignore` pattern syntax, including extension matches, directory-only matches, negation with `!`, and `**` recursive matches.

A directory-only pattern matches only when `is_directory` is true.

Negated patterns produce `included`; non-negated matching patterns produce `ignored`.

Later matching pattern rules override earlier matching pattern rules.

Pattern evaluation performs no filesystem I/O and does not inspect symlinks or special file types. It only evaluates supplied path strings and `is_directory`.

## Errors
Invalid pattern text returns `invalid_pattern`.

Malformed paths return `invalid_path`.

Pattern evaluation does not return I/O errors.

## Anchoring
`PatternLine`, invalid pattern text, and gitignore pattern syntax are anchored in `ignore.md` "Pattern Format" and the gitignore pattern syntax documented by Git.

Extension matches, directory-only matches, negation with `!`, and `**` recursive matches are anchored in examples such as `*.log`, `build/`, `!important.log`, and `**/temp`.

`PatternMatchInput.is_directory` is anchored in directory-only patterns such as `build/`.

Later-pattern override behavior and negated re-inclusion are anchored in `ignore.md` "Configuration" and "Hierarchy".

The no-I/O boundary is anchored in `decomposition.md` "gitignore-matcher".
