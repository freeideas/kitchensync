# 002_peer-startup-and-identity: Peer startup and identity

## Behavior
This concern derives from `specs/sync.md` sections "Peers", "Fallback URLs",
"Per-URL Settings", "Canon Peer (`+`)", "Subordinate Peer (`-`)", "Startup",
and "Errors", and `specs/concurrency.md` sections "Fallback URLs" and
"Connection Establishment". It covers how accepted peer arguments are grouped
into peers, how fallback URLs are tried, how roots are created or rejected, how
reachable and unreachable peers affect startup, how first sync and canon rules
are enforced, and how peers without snapshot history become subordinate.

## Notes
This category owns peer selection before traversal starts. URL normalization
belongs to `005_path-time-and-url-formats`. Scheme-specific filesystem and
SFTP operations belong to `003_peer-transports`.

## $REQ_IDs
