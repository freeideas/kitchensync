# Subordinate Peers

Behavior of subordinate (`-` prefix) and auto-subordinate peers.

## $REQ_SUB_001: Subordinate Does Not Vote
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

A subordinate peer's entries are not included in `gather_states` -- decisions are made as if the subordinate doesn't exist.

## $REQ_SUB_002: Subordinate Brought Into Conformance
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

After decisions are made, subordinate peers are brought into conformance: unwanted files are displaced, missing files are copied, directories are created or removed to match the group's decided state.

## $REQ_SUB_003: Subordinate Participates in Listing
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

Subordinate peers participate in listing and their entries are included in the union of names, but they do not contribute to decisions.

## $REQ_SUB_004: Subordinate Snapshot Updated
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

A subordinate peer's snapshot is downloaded, updated, and uploaded. On future runs without `-`, the peer participates normally as a bidirectional peer.

## $REQ_SUB_005: Auto-Subordinate on Missing Snapshot
**Source:** ./specs/algorithm.md (Section: "Subordinate Peers")

Any peer without a snapshot is automatically treated as subordinate -- it receives the group's state without influencing decisions. The canon peer is exempt from auto-subordination.

## $REQ_SUB_006: Auto-Subordinate on Failed Snapshot Download
**Source:** ./specs/algorithm.md (Section: "Startup")

If snapshot download fails (corrupt, permission denied, I/O error), the peer is treated as a new peer with an empty snapshot and becomes auto-subordinate (unless it is the canon peer).

## $REQ_SUB_007: DELETE_SUBORDINATES_ONLY
**Source:** ./specs/algorithm.md (Section: "Combined-Tree Walk")

When no contributing peer has an entry but subordinates do, the entry is displaced from subordinates only. Contributing peer snapshots are not modified.
