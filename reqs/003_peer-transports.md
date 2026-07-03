# 003_peer-transports: Peer transport operations

## Behavior
This concern derives from `specs/sync.md` sections "Authentication (fallback
chain)", "Peer Transports", "Required Operations", "Error Semantics", "Case
Sensitivity", and "Testability", `specs/concurrency.md` section "Connection
Establishment", and `extart/ephemeral-sftp-server.py`. It covers the observable
local filesystem and SFTP peer operation surface, SSH authentication order,
known-host rejection of unknown hosts, remote and local root access,
transport-neutral error categories, omission of symbolic links and special
files from listings and stats, and preservation of reported filenames.

## Notes
This category is bounded by the transport API behavior visible to the sync
engine. Copy scheduling and replacement staging belong to
`008_copy-queue-and-concurrency` and `009_recoverable-staging`.

## $REQ_IDs
