# 02_directory-actions: per-peer actions for directory (and directory-wins) decisions

## Behavior
For a `directory` or `type_conflict_directory_wins` decision, the record carries one action per active peer, drawn from a closed set covering creation, file-displacement-then-create, recursing into an already-present directory, and no-row cases. Directory decisions are existence-based — they consider whether a directory is present at the peer, not its `mod_time`. Derived from `./specs/SPEC.md` §"API surface" — per-peer action list under `directory` / `type_conflict_directory_wins`, and §"Anchoring" Directory Decisions entry.

## $REQ_IDs
- `02.19` — Every per-peer action in a `directory` or `type_conflict_directory_wins` decision is one of: `create_directory`, `displace_existing_file_then_create`, `displace_directory`, `recurse_only`, `no_action_no_row`.
- `02.20` — A peer whose listing shows a directory at this name receives `recurse_only` (the directory already exists, just descend into it).
- `02.21` — A peer whose listing is `absent` and has no conflicting entry receives `create_directory`.
- `02.22` — In a `type_conflict_directory_wins` decision, a peer whose listing shows a regular file at this name receives `displace_existing_file_then_create`.
- `02.23` — A peer whose listing is `absent` and whose snapshot row is `none` receives `no_action_no_row`.
- `02.24` — A directory decision is determined by the presence or absence of directory listings across peers, not by any `mod_time` comparison.
