# Type Conflict Decision Rules

## Purpose
Decide the authoritative entry type and required displacement outcome when peers report file and directory live entries for one path.

## Public API
Data shapes:

- `PeerRole`: `canon`, `subordinate`, or `bidirectional`
- `EntryType`: `file` or `directory`
- `LiveEntry`: `entry_type`, `mod_time`, `byte_size`
- `SnapshotRow`: `mod_time`, `byte_size`, `last_seen`, `deleted_time`
- `PeerEntryState`: `peer_id`, `role`, optional `live_entry`, optional `snapshot_row`
- `Decision`: `entry_type` optional, `winner_peer_id` optional, `target_peer_ids`, `delete_peer_ids`, `create_directory_peer_ids`, `displace_peer_ids`, `reason`

Operations:

- `decide_type_conflict(peer_states, timestamp_tolerance_seconds) -> Decision`

`peer_states` contains all peers relevant to one relative path. Subordinate peers may be present in `peer_states`, but they do not contribute votes.

## Behavior
If a canon peer has a live entry at the path, the canon peer's `entry_type` wins.

If no canon peer has a live entry at the path, `file` wins over `directory`.

Peers whose live entry has the losing type are marked for displacement.

When `file` wins, the winning file is selected using file decision rules over file entries only. Directory live entries do not contribute to file winner selection.

When `directory` wins, directory `mod_time` is not used. The directory should exist on peers that lack the winning directory after losing-type displacement.

Subordinate peers receive the resulting outcome but do not affect type selection or winner selection.

Decisions describe required outcomes only. They do not perform listing, copying, directory creation, displacement, snapshot mutation, logging, or transport operations.

## Errors
Invalid peer state returns `invalid_peer_state`.

A timestamp that cannot be compared returns `invalid_timestamp`.

A negative timestamp tolerance returns `invalid_tolerance`.

## Anchoring
`PeerRole`, canon behavior, subordinate behavior, and contributing-peer voting are anchored in `sync.md` "Peers", "Canon Peer", "Subordinate Peer", and `multi-tree-sync.md` "Subordinate Peers".

`LiveEntry`, `EntryType`, `mod_time`, and `byte_size` are anchored in `sync.md` "Peer Transports" and `multi-tree-sync.md` "Entry Classification".

`SnapshotRow`, `last_seen`, and `deleted_time` are anchored in `database.md` "Schema" and `multi-tree-sync.md` "Snapshot Updates".

`Decision`, target peers, deletion peers, directory creation peers, displacement peers, winner peer, and type conflicts are anchored in `multi-tree-sync.md` "Algorithm", "Decision Rules", "Directory Decisions", and "Type Conflicts".

`timestamp_tolerance_seconds` is anchored in `multi-tree-sync.md` "Decision Rules".

The pure-function boundary is anchored in `decomposition.md` "decision-engine".
