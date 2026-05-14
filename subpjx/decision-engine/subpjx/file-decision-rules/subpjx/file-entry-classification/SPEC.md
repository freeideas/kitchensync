# File Entry Classification

## Purpose
Classify one peer's file entry state from live file state and snapshot row.

## Public API
Data shapes:

- `PeerRole`: `canon`, `subordinate`, or `bidirectional`
- `EntryType`: `file` or `directory`
- `LiveEntry`: `entry_type`, `mod_time`, `byte_size`
- `SnapshotRow`: `mod_time`, `byte_size`, `last_seen`, `deleted_time`
- `PeerEntryState`: `peer_id`, `role`, optional `live_entry`, optional `snapshot_row`
- `Classification`: `unchanged`, `modified`, `new`, `deleted`, `absent_unconfirmed`, or `no_opinion`

Operations:

- `classify_file(peer_state, timestamp_tolerance_seconds) -> Classification`

## Behavior
`classify_file` compares a peer's live file state with that peer's snapshot row.

A live file with a snapshot row whose `deleted_time` is null is `unchanged` when its `mod_time` is within the timestamp tolerance of the snapshot `mod_time`; otherwise it is `modified`.

A live file with a tombstone snapshot row is `modified`.

A live file with no snapshot row is `new`.

An absent file with a tombstone snapshot row is `deleted`.

An absent file with a snapshot row whose `deleted_time` is null is `absent_unconfirmed`.

An absent file with no snapshot row is `no_opinion`.

## Errors
Invalid peer state returns `invalid_peer_state`.

A file classification request containing a directory live entry returns `invalid_entry_type`.

A timestamp that cannot be compared returns `invalid_timestamp`.

A negative timestamp tolerance returns `invalid_tolerance`.

## Anchoring
`PeerRole` is anchored in `sync.md` "Peers", "Canon Peer", "Subordinate Peer", and `multi-tree-sync.md` "Subordinate Peers".

`LiveEntry`, `EntryType`, `mod_time`, and `byte_size` are anchored in `sync.md` "Peer Transports" and `multi-tree-sync.md` "Entry Classification".

`SnapshotRow`, `last_seen`, and `deleted_time` are anchored in `database.md` "Schema" and `multi-tree-sync.md` "Snapshot Updates".

`PeerEntryState` is anchored in peer state, live entry, and snapshot row terms.

`Classification` is anchored in `multi-tree-sync.md` "Entry Classification".

`timestamp_tolerance_seconds` is anchored in `multi-tree-sync.md` "Decision Rules".
