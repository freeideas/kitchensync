# 03_decision-rules: winning content selected by mod_time with size and existence tiebreakers

## Behavior
For `file` and `type_conflict_file_wins` decisions, `decide` selects the winning peer's content from among the voting peers using the decision rules anchored in `multi-tree-sync.md` §"Decision Rules". The newer `mod_time` wins (with the tolerance window applied), `byte_size` acts as the tiebreaker when peers' `mod_time` values fall within tolerance of one another, and a peer that observes the entry live overrides a peer whose snapshot row is tombstoned (existence over deletion). Rule 4b specifically reconciles an `absent_unconfirmed` peer against the maximum `mod_time` observed across the other peers. Derived from `./specs/SPEC.md` §"Anchoring" entries for Decision Rules 1–6 (tie-breaking on size, existence-over-deletion) and Rule 4b.

## $REQ_IDs
- `03.12` — Among the voting peers, the peer with the latest `mod_time` (treating values within `timestamp_tolerance_seconds` as equal) is chosen as `winning_source_peer_id`, and its `mod_time` and `byte_size` are returned as `winning_mod_time` and `winning_byte_size`.
- `03.13` — When two or more peers' `mod_time` values fall within `timestamp_tolerance_seconds` of one another, `byte_size` is used as the tiebreaker when selecting the winning content.
- `03.14` — A peer that observes the entry live overrides a peer whose snapshot row carries a non-null `deleted_time` when the two would otherwise tie (existence over deletion).
- `03.15` — An `absent_unconfirmed` peer is reconciled against the maximum `mod_time` observed across the other peers (Rule 4b).
