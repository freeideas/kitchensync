# 03_canon-peer: Canon peer (`+`) behavior

## Behavior

A `+`-prefixed canon peer is authoritative — its state wins all conflicts unconditionally, overriding mod_time and snapshot history. Applies to both files and directories. Derived from `specs/sync.md` §"Canon Peer" and `specs/multi-tree-sync.md` §"Decision Rules — With a canon peer".

## $REQ_IDs
- `03.2` — When the canon peer has an entry at a path, the entry is propagated to all other peers (including subordinate peers) regardless of their snapshot rows or mod_times.
- `03.3` — When the canon peer lacks an entry at a path that some other peer has, the entry is displaced to BAK/ on every peer that has it.

## Notes
Startup canon checks (canon unreachable, first-sync-no-canon) are in `02_startup-connect.md`; the "at most one `+`" argument check is in `01_arg-validation.md`; canon's role in type conflicts is in `03_directory-decisions.md`.
