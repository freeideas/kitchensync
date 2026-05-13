# 03_peer-connect: Peer root auto-creation during connection

## Behavior

Before a peer URL is considered to have connected successfully, the program ensures the peer's root path exists on the target — creating it (along with any missing parents) if needed. This applies to both `file://` and `sftp://` peers. If creation fails, the URL is treated as a failed connection: the next fallback URL is tried, or the peer becomes unreachable if no fallback remains. Derived from `sync.md` §Startup and `concurrency.md` §"Connection Establishment".

## $REQ_IDs

- `03.86` — During connection establishment, if the peer's root path does not exist on the target, it is created (along with any missing parents) for both `file://` and `sftp://` peers before the URL is considered successfully connected.
- `03.87` — If creating the peer's root path fails during connection establishment, that URL is treated as a failed connection (the next fallback URL is tried, or the peer is unreachable if no fallback remains).
- `03.93` — The source code that establishes startup peer connections issues each peer's connection attempt via a concurrent join/gather/parallel construct, not in a sequential loop that awaits one peer's connection before starting the next.

## Notes

Other failure conditions that mark a URL as failed are covered in `03_sftp-pool.md` (handshake timeout) and `03_sftp-auth.md` (unknown host key). The peer-unreachable outcome when all URLs fail is in `03_fallback-urls.md` (03.55).
