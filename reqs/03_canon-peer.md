# 03_canon-peer: Canon peer is authoritative

## Behavior

A peer marked with the `+` prefix is the canon peer: its state wins all conflicts unconditionally for the run. Derived from `sync.md` §"Canon Peer (+)" and `multi-tree-sync.md` §"Decision Rules" and §"Directory Decisions".

## $REQ_IDs

- `03.15` — When a canon peer has a file, that file is copied to every other peer regardless of mod_time, size, or snapshot history.
- `03.16` — When a canon peer lacks a file, the file is displaced to BAK/ on every other peer that has it.
- `03.17` — When a canon peer has a directory, the directory is created on every other peer that lacks it.
- `03.40` — When a canon peer lacks a directory, the directory is displaced to BAK/ on every other peer that has it.

## Notes

Canon overriding a file-vs-directory type conflict is in `03_type-conflicts.md` (03.38). The canon-unreachable abort is in `04_error-handling.md` (04.9). The "at most one canon peer per run" argument check is in `01_cli-validation.md` (01.11). The first-sync requirement that canon is needed when no snapshots exist lives in `02_first-sync.md`.
