# Non-Canon File Outcome Decision

## Purpose
Decide the authoritative file outcome for one path from classified per-peer file entry states when no canon peer supplies the outcome.

## Public API
Data shapes:

- `PeerRole`: `subordinate` or `bidirectional`
- `EntryType`: `file` or `directory`
- `LiveEntry`: `entry_type`, `mod_time`, `byte_size`
- `SnapshotRow`: `mod_time`, `byte_size`, `last_seen`, `deleted_time`
- `Classification`: `unchanged`, `modified`, `new`, `deleted`, `absent_unconfirmed`, or `no_opinion`
- `PeerDecisionState`: `peer_id`, `role`, `classification`, optional `live_entry`, optional `snapshot_row`
- `Decision`: `entry_type` optional, `winner_peer_id` optional, `target_peer_ids`, `delete_peer_ids`, `displace_peer_ids`, `reason`

Operations:

- `decide_non_canon_file(peer_states, timestamp_tolerance_seconds) -> Decision`

`peer_states` contains all peers relevant to one relative path. Subordinate peers may be present in `peer_states`, but they do not contribute votes.

## Behavior
Only contributing peers vote. Subordinate peers receive the resulting outcome but do not affect winner selection.

If all contributing peers are `unchanged`, the decision takes no action.

If any contributing peer is `modified`, the newest `mod_time` wins. Peers whose `mod_time` is within the timestamp tolerance of the maximum are tied.

If any contributing peer has a `new` file, the newest `mod_time` wins and peers that lack the file are targets.

For `deleted` plus existing file states, the deletion estimate is the most recent `deleted_time` or qualifying `last_seen` among deleting peers. If the deletion estimate exceeds the existing file `mod_time` by more than the timestamp tolerance, deletion wins. Otherwise the existing file wins.

For `absent_unconfirmed`, `last_seen` must exceed the maximum live file `mod_time` by more than the timestamp tolerance to become a deletion vote. If `last_seen` is null or does not exceed the maximum live file `mod_time`, the absence is treated as a failed or incomplete copy and does not vote for deletion.

When live files are tied by `mod_time`, larger `byte_size` wins. Remaining ties keep data: existence wins over deletion, and larger files win over smaller files.

Peers with `no_opinion` do not vote. If no contributing peer votes, the entry does not exist in the group view, and subordinate peers that have it are marked for displacement.

Decisions describe required outcomes only. They do not perform listing, copying, directory creation, displacement, snapshot mutation, logging, or transport operations.

## Errors
Invalid peer state returns `invalid_peer_state`.

A file decision request containing directory live entries returns `invalid_entry_type`.

A timestamp that cannot be compared returns `invalid_timestamp`.

A negative timestamp tolerance returns `invalid_tolerance`.

## Anchoring
`PeerRole`, subordinate behavior, and contributing-peer voting are anchored in `sync.md` "Peers", "Subordinate Peer", and `multi-tree-sync.md` "Subordinate Peers".

`LiveEntry`, `EntryType`, `mod_time`, and `byte_size` are anchored in `sync.md` "Peer Transports" and `multi-tree-sync.md` "Entry Classification".

`SnapshotRow`, `last_seen`, and `deleted_time` are anchored in `database.md` "Schema" and `multi-tree-sync.md` "Snapshot Updates".

`Classification` is anchored in `multi-tree-sync.md` "Entry Classification".

`PeerDecisionState` is anchored in peer state, peer role, live entry, snapshot row, and classification terms.

`Decision`, target peers, deletion peers, displacement peers, and winner peer are anchored in `multi-tree-sync.md` "Algorithm" and "Decision Rules".

`timestamp_tolerance_seconds` is anchored in `multi-tree-sync.md` "Decision Rules".
