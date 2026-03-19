# Canon Mode

Behavior when `--canon <peer>` is specified, making one peer authoritative.

## $REQ_CANON_002: Canon Has File
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the canon peer has a file, it is pushed to all other peers.

## $REQ_CANON_003: Canon Lacks File
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the canon peer lacks a file, it is deleted (displaced) on all other peers.

## $REQ_CANON_004: Canon Unreachable Exits Error
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the canon peer is unreachable, the application exits with an error at startup.

## $REQ_CANON_005: Canon Single Peer Sufficient
**Source:** ./specs/sync.md (Section: "Startup")

With `--canon`, one reachable peer (the canon peer itself) is sufficient to run — the normal two-peer minimum does not apply.

## $REQ_CANON_006: Canon Snapshot Update for Offline Peers
**Source:** ./specs/sync.md (Section: "Startup")

With `--canon` and only the canon peer reachable, the snapshot is updated from the canon peer's state so that when other peers come online, sync can detect and propagate bidirectional changes rather than treating everything as new.
