# Gitignore Matcher

## Purpose
Compile gitignore-style pattern text into a pure path matcher.

## Public API
Data shapes:

- `PatternSet`: ordered pattern lines with a `base_path`
- `Matcher`: compiled pattern set hierarchy
- `MatchInput`: relative `path`, `is_directory`
- `MatchResult`: `ignored` or `included`

Operations:

- `compile(pattern_sets) -> Matcher`
- `matches(matcher, input) -> MatchResult`

## Behavior
Patterns use `.gitignore` syntax, including extension matches, directory-only matches, negation with `!`, and `**` recursive matches.

Pattern sets are evaluated in hierarchy order. Deeper pattern sets add to and may override patterns from parent directories.

A pattern applies only to the directory represented by its `base_path` and that directory's descendants.

Later matching patterns override earlier matching patterns. Negated patterns re-include a path previously ignored by an earlier pattern.

The matcher performs no filesystem I/O and does not inspect symlinks or special file types. It only evaluates supplied path strings and `is_directory`.

## Errors
Invalid pattern text returns `invalid_pattern`.

Malformed paths return `invalid_path`.

The matcher does not return I/O errors.

## Anchoring
`PatternSet`, hierarchy ordering, parent and child rule accumulation, and override behavior are anchored in `ignore.md` "Configuration" and "Hierarchy".

Gitignore pattern syntax, including `*.log`, `build/`, `!important.log`, and `**/temp`, is anchored in `ignore.md` "Pattern Format" and the gitignore pattern syntax documented by Git.

`MatchInput.is_directory` is anchored in directory-only patterns such as `build/`.

The no-I/O boundary is anchored in `decomposition.md` "gitignore-matcher".
