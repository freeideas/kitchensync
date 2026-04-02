# Instance Lock

Prevents multiple KitchenSync instances from operating on overlapping peers simultaneously. Uses an ephemeral TCP port as a live proof-of-ownership -- no PID files, no stale-lock cleanup.

## Why Not PID Files

PIDs are reused quickly by the OS. A stale PID file whose number happens to match a new unrelated process is indistinguishable from a live lock. Checking process names or command lines is fragile and platform-dependent. The ephemeral-port approach avoids all of this: if the port responds with the right data, the instance is alive. If the connection is refused, it's gone.

## Mechanism

### On Startup

1. Bind a TCP listener on `127.0.0.1:0` (OS assigns a random high port)
2. Serve a single endpoint that returns the instance's canonical peer list (see [Lock Endpoint](#lock-endpoint))
3. Write the port number into each local peer's `.kitchensync/lock` file (plain text, just the port number, no newline). For SFTP peers, write via the SFTP connection
4. Proceed with sync

### Before Starting (Instance Check)

Before binding the port, check every peer for an existing lock:

1. For each peer, read `.kitchensync/lock`. If the file does not exist or is empty, that peer is clear -- continue
2. If a port number is found, POST `http://127.0.0.1:{port}/instance-peers`
3. If the connection succeeds and the response contains **any peer that overlaps** with the current instance's peer list, print the overlapping peers and exit 1
4. If the connection is refused, times out, or returns an error, the old instance is gone. Delete the stale `.kitchensync/lock` file and continue

Peer comparison uses OS-canonicalized paths (resolving symlinks, `.`/`..`, and normalizing separators). For SFTP peers, the comparison key is `user@host:port/path` with the path canonicalized.

The check must examine **all** peers before deciding. If any single peer reports an overlap, the instance must not start.

### Lock Endpoint

**POST /instance-peers** -- returns a JSON array of canonical peer identifiers, sorted. Example:

```json
[
  "file:///c:/photos",
  "sftp://bilbo@cloud:22/volume1/photos"
]
```

The listener is bound to `127.0.0.1` only -- not reachable from the network.

### On Shutdown

1. Delete `.kitchensync/lock` from every peer (best-effort -- connection failures are logged but do not prevent exit)
2. Close the TCP listener

Lock cleanup happens regardless of whether shutdown is clean (normal exit, SIGTERM) or unclean (crash, SIGKILL). If cleanup is missed, the next instance detects connection-refused and cleans up the stale file.

## Watch Mode

In `--watch` mode the lock listener remains open for the duration of the session. The lock file stays in place. Other instances attempting to sync any overlapping peer will detect the running watcher and exit.

## Edge Cases

- **Peer unreachable during lock check**: if a peer cannot be reached to read its `.kitchensync/lock` file (SFTP connection failure, local path not mounted), treat it as clear. The sync will fail later for the same reason -- no need to block on lock checks for unreachable peers.
- **Lock file contains garbage**: treat as no lock (delete it and continue).
- **Race between two instances starting simultaneously**: both read locks before either writes. Both proceed. The OS prevents both from binding the same port, but they bind different ports. Both write their lock files. Neither detects the other. This is a narrow window. To close it: after writing lock files, re-read all peers' lock files and verify your own port is the one written. If another port appears, the instance with the lower port number wins; the other exits 1.
- **Single-peer mode**: the lock still applies. Even a snapshot-only run should not collide with another instance operating on the same peer.
- **Cross-machine concurrency**: Instance locking protects against concurrent instances on the same machine. Cross-machine concurrency for shared SFTP peers is not detected; users should coordinate externally.
