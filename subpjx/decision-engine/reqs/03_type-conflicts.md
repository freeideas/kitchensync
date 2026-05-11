# 03_type-conflicts: file wins by default when peers disagree on file vs directory

## Behavior
When some active peers report a regular file at the entry name and other active peers report a directory at the same name, `decide` resolves the disagreement as a type conflict. The default resolution is "file wins" — `type_conflict_file_wins` — under which peers carrying a directory at this name are scheduled to displace it so the file representation takes its place. A `canon` peer can override the default and force `type_conflict_directory_wins` instead. Derived from `./specs/SPEC.md` §"API surface" decision-kinds list and §"Anchoring" Type Conflicts entry.

## $REQ_IDs
- `03.1` — When active peers disagree on whether the entry is a regular file or a directory, the decision's `kind` is `type_conflict_file_wins` or `type_conflict_directory_wins`.
- `03.2` — With no `canon` peer dictating otherwise, a file-vs-directory disagreement resolves to `type_conflict_file_wins`.
- `03.4` — A `canon` peer reporting a directory at this name causes the conflict to resolve to `type_conflict_directory_wins`.
