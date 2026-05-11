# 02_file-actions: per-peer actions for file (and file-wins) decisions

## Behavior
For a `file` or `type_conflict_file_wins` decision, the record carries one action per active peer, drawn from a closed set. The action tells the caller what to do at that peer's path: copy the winning data in, confirm that the peer already matches, displace a conflicting entry, or do nothing because there is no existing row to update. Derived from `./specs/SPEC.md` §"API surface" — the per-peer action list under `file` / `type_conflict_file_wins`.

## $REQ_IDs
- `02.14` — Every per-peer action in a `file` or `type_conflict_file_wins` decision is one of: `copy_from_winner`, `already_matches`, `displace_existing_file`, `displace_existing_directory`, `displace_then_copy`, `no_action_no_row`.
- `02.15` — A peer whose live-file listing already matches the winning `mod_time` and `byte_size` receives `already_matches`.
- `02.16` — A peer that needs the winning content and has no conflicting file or directory at this name receives `copy_from_winner`.
- `02.18` — A peer whose listing is `absent` and whose snapshot row is `none` receives `no_action_no_row`.
