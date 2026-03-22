# Startup Sequence

Peer connection, snapshot download, and auto-subordination during startup.

## $REQ_STARTUP_001: Parallel Peer Connection
**Source:** ./specs/sync.md (Section: "Startup")

At startup, all peers are connected to in parallel.

## $REQ_STARTUP_002: Auto-Create Peer Root Directory
**Source:** ./specs/sync.md (Section: "Startup")

If a peer's root directory does not exist, it is automatically created (including any missing parents) for both `file://` and `sftp://` URLs.

## $REQ_STARTUP_003: Fallback URL Connection Order
**Source:** ./specs/sync.md (Section: "Startup")

For peers with fallback URLs (bracket syntax), URLs are tried in order; the first that connects wins.

## $REQ_STARTUP_004: Unreachable Peer Warning
**Source:** ./specs/sync.md (Section: "Startup")

Unreachable peers are skipped with a warning logged.

## $REQ_STARTUP_013: Directory Creation Failure Treats Peer as Unreachable
**Source:** ./specs/sync.md (Section: "Startup")

If directory creation fails during connection, the peer is treated as unreachable (the next fallback URL is tried).

## $REQ_STARTUP_005: Minimum Two Reachable Peers
**Source:** ./specs/sync.md (Section: "Startup")

If fewer than two peers are reachable, the program exits with an error.

## $REQ_STARTUP_006: Canon Peer Must Be Reachable
**Source:** ./specs/sync.md (Section: "Startup")

If the canon peer (`+`) is unreachable, the program exits with an error.

## $REQ_STARTUP_007: Snapshot Download
**Source:** ./specs/sync.md (Section: "Startup")

Each peer's `.kitchensync/snapshot.db` is downloaded to a local temp directory (`{tmp}/{uuid}/snapshot.db`). If a peer has no `snapshot.db`, a new empty one is created locally.

## $REQ_STARTUP_009: No Snapshots Without Canon Error
**Source:** ./specs/sync.md (Section: "Startup")

If no peer has any snapshot data and no canon peer (`+`) is designated, the program exits with an error.

## $REQ_STARTUP_010: First Sync Suggestion Message
**Source:** ./specs/sync.md (Section: "Canon Peer (+)")

On a first run with no canon, the program prints: `First sync? Mark the authoritative peer with a leading +`

## $REQ_STARTUP_011: No Contributing Peer Reachable Error
**Source:** ./specs/sync.md (Section: "Startup")

If no contributing (non-subordinate) peer is reachable after auto-subordination, the program exits with error: `No contributing peer reachable — cannot make sync decisions`

## $REQ_STARTUP_012: Snapshotless Peer as Automatic Subordinate
**Source:** ./specs/sync.md (Section: "Subordinate Peer (-)")

Any peer without a snapshot (no `.kitchensync/snapshot.db`) is automatically treated as subordinate, unless it is the canon peer (`+`). The `-` prefix is redundant for snapshotless peers but harmless.

