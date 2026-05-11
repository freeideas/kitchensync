# 02_classify-file: entry classification returns one of seven defined values

## Behavior
`classify_file` examines a single peer's listing observation for an entry and the matching snapshot row, and returns one classification value describing that peer's state for the entry. The classification distinguishes confirmed-unchanged entries, modified entries, resurrections (live again after a tombstone), new entries (live with no prior row), confirmed deletions (snapshot already tombstoned), unconfirmed absences (newly missing where the prior row was live), and no-opinion states (peer has no prior row). Derived from `./specs/SPEC.md` §"API surface" `classify_file` paragraph and §"Anchoring" Entry Classification entry.

## $REQ_IDs
- `02.1` — The returned classification is one of: `unchanged`, `modified`, `resurrection`, `new`, `deleted`, `absent_unconfirmed`, `no_opinion`.
- `02.2` — A live-file listing whose `mod_time` matches the peer's snapshot row's `mod_time` (within `timestamp_tolerance_seconds`), and whose snapshot row's `deleted_time` is null, classifies as `unchanged`.
- `02.3` — A live-file listing whose `mod_time` differs from the peer's snapshot row's `mod_time` (beyond tolerance), and whose snapshot row's `deleted_time` is null, classifies as `modified`.
- `02.4` — A live listing on a peer whose snapshot row carries a non-null `deleted_time` classifies as `resurrection`.
- `02.5` — A live listing on a peer that has no snapshot row classifies as `new`.
- `02.6` — An `absent` listing on a peer whose snapshot row carries a non-null `deleted_time` classifies as `deleted` and carries an estimate value equal to that `deleted_time`.
- `02.7` — An `absent` listing on a peer whose snapshot row's `deleted_time` is null classifies as `absent_unconfirmed`.
- `02.33` — An `absent` listing on a peer that has no snapshot row classifies as `no_opinion`.

## Notes
`classify_file` is exposed publicly so callers can log or test classification independently; `decide` uses it internally over the per-peer inputs.
