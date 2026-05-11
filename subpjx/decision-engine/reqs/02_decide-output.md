# 02_decide-output: decide returns a decision record describing what happens at the entry's path

## Behavior
`decide` returns a single decision record summarising the outcome at one entry name across all active peers. The record's `kind` field is one of `file`, `directory`, `type_conflict_file_wins`, `type_conflict_directory_wins`, or `noop`. For `file` and `type_conflict_file_wins` decisions the record additionally carries the winning content metadata — `winning_mod_time`, `winning_byte_size`, and `winning_source_peer_id` (the peer from which the caller should read the data). A `noop` decision is returned only when every peer either already matches the group's view or has no row. Derived from `./specs/SPEC.md` §"API surface" decision-output paragraph.

## $REQ_IDs
- `02.8` — The returned decision's `kind` is one of: `file`, `directory`, `type_conflict_file_wins`, `type_conflict_directory_wins`, `noop`.
- `02.11` — When every peer either already matches the group's view or has no snapshot row, the decision's `kind` is `noop`.
- `02.12` — A `file` or `type_conflict_file_wins` decision carries `winning_mod_time`, `winning_byte_size`, and `winning_source_peer_id`.
