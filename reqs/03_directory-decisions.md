# 03_directory-decisions: Directory existence and type conflicts

## Behavior

Directory decisions are existence-based — mod_time is not used. A directory is created on peers that lack it if any contributing peer has it; it is displaced everywhere if all contributing peers with a snapshot row have tombstoned it. Type conflicts resolve to file (or to canon's type if canon has an entry), with the losing form displaced. Derived from `specs/multi-tree-sync.md` §"Directory Decisions" and §"Type Conflicts".

## $REQ_IDs
- `03.12` — If any contributing peer has a directory at a path, that directory is created on every peer that lacks it (including subordinate peers).
- `03.13` — If every contributing peer with a snapshot row for the directory has tombstoned it (and none have it live), the directory is displaced to BAK/ on every peer that still has it.
- `03.14` — A contributing peer with no snapshot row for the directory has no opinion and does not block deletion.
- `03.15` — When the same path is a file on one peer and a directory on another and no canon peer has an entry there, the file wins and the directory is displaced to BAK/ on peers that have it.
- `03.16` — When the canon peer has an entry at a path involved in a type conflict, the canon peer's type wins and the losing type is displaced to BAK/ on peers that have it.
- `03.17` — A displaced directory is renamed as a single same-filesystem rename to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, preserving its entire subtree.
