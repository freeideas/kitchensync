# 03_directory-decisions: Existence-based decisions for directories

## Behavior

Directories are decided by existence, not mod_time. A directory present on any contributing peer must exist on every peer; a directory present in no contributing listing but tombstoned on all contributing peers that ever knew it is deleted from every peer that still has it. Derived from `multi-tree-sync.md` §"Directory Decisions".

## $REQ_IDs

- `03.9` — When any contributing peer has a directory, the directory is created on every peer that lacks it.
- `03.10` — When every contributing peer that has a snapshot row for a directory has a tombstone for it and none has it live, the directory is displaced to BAK/ on every remaining peer that still has it.
- `03.11` — A contributing peer with no snapshot row for a directory does not block deletion.
- `03.12` — When no contributing peer has a directory live or in any snapshot row, subordinate peers that have it are displaced to BAK/.
- `03.13` — Directory mod_times are not used to choose between peers when deciding a directory's existence.

## Notes

Canon-peer overrides for directories follow the canon-peer file rules: canon has it → create everywhere; canon lacks it → delete everywhere. See `03_canon-peer.md`.
