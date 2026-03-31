# Peer Modes

Subordinate peers, single-peer snapshot mode, and offline peer handling.

## $REQ_PEER_001: Subordinate Does Not Vote
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

A subordinate peer (`-` prefix) does not contribute to decisions — its entries are not included in `gather_states`. Decisions are made as if it doesn't exist.

## $REQ_PEER_002: Subordinate Brought into Conformance
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

After decisions are made, subordinate peers are brought into conformance: unwanted files displaced, missing files copied, directories created/removed.

## $REQ_PEER_003: Subordinate Snapshot Maintained
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

A subordinate peer's snapshot is downloaded, updated during the walk, and uploaded back. On future runs without the `-` prefix, it participates normally.

## $REQ_PEER_004: Auto-Subordinate No Snapshot
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

Any peer without a snapshot is automatically treated as subordinate (unless it's the canon peer).

## $REQ_PEER_005: Single-Peer Snapshot Only
**Source:** ./specs/algorithm.md (Section: "Startup")

Running with a single peer records a snapshot (present files get `last_seen = now`, absent files get tombstoned) without performing any sync operations.

## $REQ_PEER_006: Offline Peer Excluded
**Source:** ./specs/algorithm.md (Section: "Offline Peers")

Unreachable peers are excluded entirely — they do not participate in listings or decisions. Their snapshot rows are not modified.

## $REQ_PEER_007: Offline Peer Non-Fatal
**Source:** ./specs/algorithm.md (Section: "Offline Peers")

Failure to connect to one peer is non-fatal — exit 0 if at least one sync completes.

## $REQ_PEER_008: Offline Peer Catches Up
**Source:** ./specs/algorithm.md (Section: "Offline Peers")

On the next run when a previously offline peer is reachable, discrepancies between its filesystem state and snapshot drive sync decisions, bringing it up to date.

## $REQ_PEER_009: First Sync Needs Canon
**Source:** ./specs/algorithm.md (Section: "Startup")

In multi-peer mode, if no peer has snapshot data and no canon peer is designated, the program exits 1 with a suggestion to use `+`.

## $REQ_PEER_010: No Contributing Peer Reachable
**Source:** ./specs/algorithm.md (Section: "Startup")

In multi-peer mode, if no contributing (non-subordinate with snapshot) peer is reachable, the program exits 1.
