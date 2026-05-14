# Directory Decision Rules

## Purpose
Decide the authoritative directory outcome for one path from live directory states and snapshot rows.

## Public API
Data shapes:

- `PeerRole`: `canon`, `subordinate`, or `bidirectional`
- `EntryType`: `file` or `directory`
- `LiveEntry`: `entry_type`, `mod_time`, `byte_size`
- `SnapshotRow`: `mod_time`, `byte_size`, `last_seen`, `deleted_time`
- `PeerEntryState`: `peer_id`, `role`, optional `live_entry`, optional `snapshot_row`
- `Decision`: `entry_type` optional, `winner_peer_id` optional, `target_peer_ids`, `delete_peer_ids`, `create_directory_peer_ids`, `displace_peer_ids`, `reason`

Operations:

- `decide_directory(peer_states) -> Decision`

`peer_states` contains all peers relevant to one relative path. Subordinate peers may be present in `peer_states`, but they do not contribute votes.

## Behavior
Directory `mod_time` is not used.

If any contributing peer has the directory live, the directory should exist on all peers.

If all contributing peers that have a snapshot row for the directory have deleted it and no contributing peer has it live, deletion wins.

A contributing peer with no snapshot row does not block deletion.

If no contributing peer has the directory live or in a snapshot row, the directory does not exist in the group view, and subordinate peers that have it are marked for displacement.

Decisions describe required outcomes only. They do not perform listing, copying, directory creation, displacement, snapshot mutation, logging, or transport operations.

## Errors
Invalid peer state returns `invalid_peer_state`.

A directory decision request containing file live entries returns `invalid_entry_type`.

A timestamp that cannot be compared returns `invalid_timestamp`.

## Anchoring
`PeerRole`, subordinate behavior, and contributing-peer voting are anchored in `sync.md` "Peers", "Subordinate Peer", and `multi-tree-sync.md` "Subordinate Peers".

`LiveEntry`, `EntryType`, and `mod_time` are anchored in `sync.md` "Peer Transports" and `multi-tree-sync.md` "Entry Classification".

`SnapshotRow` and `deleted_time` are anchored in `database.md` "Schema" and `multi-tree-sync.md` "Snapshot Updates".

`Decision`, directory creation peers, deletion peers, displacement peers, and directory decisions are anchored in `multi-tree-sync.md` "Algorithm", "Decision Rules", and "Directory Decisions".

The pure-function boundary is anchored in `decomposition.md` "decision-engine".
