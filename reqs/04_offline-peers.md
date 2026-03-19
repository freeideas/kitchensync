# Offline Peers

Handling of unreachable and occasionally-connected peers.

## $REQ_OFFLINE_001: Unreachable Peer Exclusion
**Source:** ./specs/multi-tree-sync.md (Section: "Offline Peers")

Unreachable peers are excluded entirely — they do not participate in listings or decisions.

## $REQ_OFFLINE_002: Snapshot Preservation for Offline Peers
**Source:** ./specs/multi-tree-sync.md (Section: "Offline Peers")

Unreachable peers' snapshot rows are not modified — `last_seen` is not updated.

## $REQ_OFFLINE_003: Reconnection Sync
**Source:** ./specs/multi-tree-sync.md (Section: "Offline Peers")

On the next run when an offline peer is reachable, discrepancies between its filesystem state and its snapshot rows drive sync decisions, bringing it up to date.

## $REQ_OFFLINE_004: Skip and Log Unreachable
**Source:** ./specs/sync.md (Section: "Startup")

On startup, unreachable peers are skipped and a warning is logged. The sync continues with reachable peers.
