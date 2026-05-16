# 03_type-conflicts: File vs directory conflicts at the same path

## Behavior

When the same path is a file on one peer and a directory on another, the conflict is resolved before any normal file decision: if a canon peer is present and has an entry at that path, its type wins; otherwise the file wins over the directory. The losing type is displaced to BAK/, and the winning file is then chosen by the standard decision rules and synced. Derived from `multi-tree-sync.md` §"Type Conflicts".

## $REQ_IDs

- `03.36` — When no canon peer is designated, or the canon peer has no entry at the conflicting path, the file wins over the directory at the same path: the directory is displaced to BAK/ on peers that have it.
- `03.37` — After the type conflict is resolved (without canon override), the winning file is propagated to all peers, including peers that previously had the directory at that path.
- `03.38` — When a canon peer is present and has an entry at the conflicting path, the canon peer's type wins: the other type is displaced to BAK/ on every peer that has it.
<!-- SPEC GAP: The Behavior summary and 03.36 say a file wins when the canon peer has no entry at the conflicting path, but multi-tree-sync.md says the canon peer's absence wins and the path is displaced everywhere else. -->
- `03.39` — When a canon peer is present and lacks a path that is a file on one peer and a directory on another, the canon peer's absence wins: files and directories at that path are displaced to BAK/ on every other peer that has them.

## Notes

Once the type is decided, the surviving file's mod_time and size are used by the normal decision rules (`03_decision-rules.md`). Single-rename subtree displacement for displaced directories is covered by 03.34 in `03_tmp-bak-staging.md`.
