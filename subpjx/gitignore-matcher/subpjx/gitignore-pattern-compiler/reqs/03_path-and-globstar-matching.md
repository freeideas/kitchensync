# 03_path-and-globstar-matching: Path-shape rule, `**` semantics, and pure-body matching

## Behavior
A pattern's shape determines how `matches(path)` interprets the candidate: when the pattern contains a `/` other than as the trailing directory marker, it is a path-shaped pattern matched against the full candidate relative path; otherwise it matches against any single path segment. The `**` wildcard has three recognised positions for directory-spanning behavior — leading, trailing, and between segments — and degenerates to single-`*` semantics when it sits next to other characters within a path segment. The `matches` predicate is purely textual: it ignores the `is_anchored` and `is_dir_only` flags, which are reported separately for the host to interpret. Derived from `specs/SPEC.md` sections "Compiling" (the `**` rules and the slash-anywhere-else rule) and "PatternSet shape" (the `matches` description).

## $REQ_IDs
- `03.1` — A pattern containing `/` anywhere except as the trailing directory marker is matched against the full forward-slash-separated candidate relative path.
- `03.2` — A pattern with no internal `/` matches when applied to any single path segment of the candidate path.
- `03.3` — A leading `**/` makes the rest of the pattern match at any directory depth.
- `03.4` — A trailing `/**` makes the pattern match anything inside the matched directory.
- `03.5` — `/**/` between segments matches zero or more intermediate directory segments.
- `03.6` — `**` adjacent to non-`/` characters within a path segment behaves as a single `*` (no directory-spanning).
- `03.7` — `CompiledPattern.matches(path)` answers a body-only textual question and ignores the `is_anchored` and `is_dir_only` flags.
