# Startup and Peer Connection

Startup sequence: connecting to peers, reachability checks, and validation before sync begins.

## $REQ_START_001: Parallel Peer Connection
**Source:** ./README.md (Section: "How Sync Works")

All peers are connected in parallel at startup. For peers with fallback URLs, URLs are tried in order; the first that connects wins.

## $REQ_START_002: Auto-Create Peer Root Directories
**Source:** ./specs/algorithm.md (Section: "Startup")

On connect, peer root directories are automatically created for both `file://` and `sftp://` peers. If root creation fails for a `file://` URL without fallbacks, the peer is logged as an error and marked unreachable.

## $REQ_START_003: Unreachable Peer Warning
**Source:** ./specs/algorithm.md (Section: "Startup")

If a peer cannot be reached on any of its URLs, a warning is logged and the peer is marked unreachable. The sync continues with remaining peers.

## $REQ_START_004: No Reachable Peers
**Source:** ./specs/algorithm.md (Section: "Startup")

If zero peers are reachable, the program exits 1 with an error message.

## $REQ_START_005: Canon Peer Must Be Reachable
**Source:** ./specs/algorithm.md (Section: "Startup")

If the canon (`+`) peer is unreachable, the program exits 1.

## $REQ_START_006: Single Reachable in Multi-Peer Mode
**Source:** ./specs/algorithm.md (Section: "Startup")

If only one peer is reachable out of two or more specified, a warning is logged and the program runs in snapshot-only mode for that peer.

## $REQ_START_007: First Sync Requires Canon
**Source:** ./specs/algorithm.md (Section: "Startup")

In multi-peer mode, if no peer has existing snapshot rows (excluding the sentinel) and no canon peer is specified, the program prints a suggestion to use `+` and exits 1.

## $REQ_START_008: No Contributing Peer Reachable
**Source:** ./specs/algorithm.md (Section: "Startup")

In multi-peer mode, if no contributing (non-subordinate) peer is reachable, the program exits 1.

## $REQ_START_009: Single-Peer Snapshot Mode
**Source:** ./README.md (Section: "Snapshot a Peer Before Taking It Offline")

Running with a single peer records a snapshot of what's there without syncing. The normal algorithm works -- decisions are trivially no-ops but snapshot updates fire correctly.

## $REQ_START_010: Auto-Subordinate New Peers
**Source:** ./specs/algorithm.md (Section: "Startup")

A peer without an existing snapshot is automatically treated as subordinate (receives the group's state without influencing decisions), unless it is the canon peer.

## $REQ_START_011: Snapshot Download Failure
**Source:** ./specs/algorithm.md (Section: "Startup")

If a peer's snapshot download fails (corrupt, permission denied, I/O error), a warning is logged, an empty snapshot is created locally, and the peer is treated as a new peer (auto-subordinate unless it is the canon peer).

**Testability:** This requirement describes internal error recovery. Testing it requires sabotaging the snapshot file, which violates testing philosophy. Do not write a test for this requirement.
