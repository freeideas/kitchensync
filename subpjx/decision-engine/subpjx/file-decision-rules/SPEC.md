# File Decision Rules

## Purpose
Classify per-peer file entry state and decide the authoritative file outcome for one path from live file states and snapshot rows.

## Public API
Data shapes:

- `PeerRole`: `canon`, `subordinate`, or `bidirectional`
- `EntryType`: `file` or `directory`
- `LiveEntry`: `entry_type`, `mod_time`, `byte_size`
- `SnapshotRow`: `mod_time`, `byte_size`, `last_seen`, `deleted_time`
- `PeerEntryState`: `peer_id`, `role`, optional `live_entry`, optional `snapshot_row`
- `Classification`: `unchanged`, `modified`, `new`, `deleted`, `absent_unconfirmed`, or `no_opinion`
- `Decision`: `entry_type` optional, `winner_peer_id` optional, `target_peer_ids`, `delete_peer_ids`, `displace_peer_ids`, `reason`

Operations:

- `classify_file(peer_state, timestamp_tolerance_seconds) -> Classification`
- `decide_file(peer_states, timestamp_tolerance_seconds) -> Decision`

`peer_states` contains all peers relevant to one relative path. Subordinate peers may be present in `peer_states`, but they do not contribute votes.

## Behavior
`classify_file` compares a contributing peer's live file state with that peer's snapshot row.

A live file with a snapshot row whose `deleted_time` is null is `unchanged` when its `mod_time` is within the timestamp tolerance of the snapshot `mod_time`; otherwise it is `modified`.

A live file with a tombstone snapshot row is `modified`.

A live file with no snapshot row is `new`.

An absent file with a tombstone snapshot row is `deleted`.

An absent file with a snapshot row whose `deleted_time` is null is `absent_unconfirmed`.

An absent file with no snapshot row is `no_opinion`.

For file decisions with a canon peer, the canon peer's live state wins unconditionally. If the canon peer has a file, the decision targets every peer that lacks a matching file. If the canon peer lacks the file, the decision deletes peers that have it.

For file decisions without a canon peer, only contributing peers vote. Subordinate peers receive the resulting outcome but do not affect winner selection.

If all contributing peers are unchanged, the decision takes no action.

If any contributing peer is modified, the newest `mod_time` wins. Peers whose `mod_time` is within the timestamp tolerance of the maximum are tied.

If any contributing peer has a new file, the newest `mod_time` wins and peers that lack the file are targets.

For deleted plus existing file states, the deletion estimate is the most recent `deleted_time` or qualifying `last_seen` among deleting peers. If the deletion estimate exceeds the existing file `mod_time` by more than the timestamp tolerance, deletion wins. Otherwise the existing file wins.

For `absent_unconfirmed`, `last_seen` must exceed the maximum live file `mod_time` by more than the timestamp tolerance to become a deletion vote. If `last_seen` is null or does not exceed the maximum live file `mod_time`, the absence is treated as a failed or incomplete copy and does not vote for deletion.

When live files are tied by `mod_time`, larger `byte_size` wins. Remaining ties keep data: existence wins over deletion, and larger files win over smaller files.

Peers with no snapshot row and no live entry do not vote. If no contributing peer votes, the entry does not exist in the group view, and subordinate peers that have it are marked for displacement.

Decisions describe required outcomes only. They do not perform listing, copying, directory creation, displacement, snapshot mutation, logging, or transport operations.

## Errors
Invalid peer state returns `invalid_peer_state`.

A file decision request containing directory live entries returns `invalid_entry_type`.

A timestamp that cannot be compared returns `invalid_timestamp`.

A negative timestamp tolerance returns `invalid_tolerance`.

## Anchoring
`PeerRole`, canon behavior, subordinate behavior, and contributing-peer voting are anchored in `sync.md` "Peers", "Canon Peer", "Subordinate Peer", and `multi-tree-sync.md` "Subordinate Peers".

`LiveEntry`, `EntryType`, `mod_time`, and `byte_size` are anchored in `sync.md` "Peer Transports" and `multi-tree-sync.md` "Entry Classification".

`SnapshotRow`, `last_seen`, and `deleted_time` are anchored in `database.md` "Schema" and `multi-tree-sync.md` "Snapshot Updates".

`Classification` is anchored in `multi-tree-sync.md` "Entry Classification".

`Decision`, target peers, deletion peers, displacement peers, and winner peer are anchored in `multi-tree-sync.md` "Algorithm" and "Decision Rules".

`timestamp_tolerance_seconds` is anchored in `multi-tree-sync.md` "Decision Rules".
