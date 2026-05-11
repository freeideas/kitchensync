# 03_peer-roles: canon overrides defaults, subordinate is non-voting

## Behavior
Each peer carries a `role` of `contributing`, `canon`, or `subordinate`. `contributing` peers vote normally in the decision. A `canon` peer's listing and snapshot can override the default group resolution — including overriding the "file wins" default of a type conflict and dictating the winning content when its observations conflict with the majority. A `subordinate` peer is non-voting: its listing and snapshot do not influence the winning kind, content, or source peer. A subordinate peer still receives a per-peer action and snapshot directive so its state is brought into line with the group's decision. Derived from `./specs/SPEC.md` §"API surface" `role` field and §"Anchoring" Canon Peer / Subordinate Peer entries.

## $REQ_IDs
- `03.5` — A peer's `role` is one of: `contributing`, `canon`, `subordinate`.
- `03.6` — A `canon` peer's observations override the default group decision (its preference dictates the winning kind and the winning content).
- `03.7` — `winning_source_peer_id` is never the `peer_id` of a `subordinate` peer.
- `03.8` — A `subordinate` peer's listing and snapshot do not contribute to deciding the `kind`, `winning_mod_time`, `winning_byte_size`, or `winning_source_peer_id`.
- `03.9` — A `subordinate` peer still receives a per-peer action and a snapshot update directive determined by the group's decision.
