# Instance Lock

Prevents multiple KitchenSync instances from operating on overlapping peers simultaneously using an ephemeral TCP port.

## $REQ_LOCK_001: Lock Listener Binding
**Source:** ./specs/instance-lock.md (Section: "On Startup")

On startup, the instance binds a TCP listener on `127.0.0.1:0` (OS-assigned random port), not reachable from the network.

## $REQ_LOCK_002: Lock File Written to Peers
**Source:** ./specs/instance-lock.md (Section: "On Startup")

The lock port number is written to `.kitchensync/lock` in each local peer's directory (plain text, just the port number, no newline). For SFTP peers, the lock file is written via the SFTP connection.

## $REQ_LOCK_003: Overlap Detection
**Source:** ./specs/instance-lock.md (Section: "Before Starting")

Before binding the port, each peer's `.kitchensync/lock` is read. If a port number is found, the instance POSTs to `http://127.0.0.1:{port}/instance-peers`. If the response contains any peer overlapping with the current instance's peer list, the program prints the overlapping peers and exits 1.

## $REQ_LOCK_004: Stale Lock Cleanup
**Source:** ./specs/instance-lock.md (Section: "Before Starting")

If the connection to a lock port is refused, times out, or returns an error, the old instance is gone. The stale `.kitchensync/lock` file is deleted and startup continues.

## $REQ_LOCK_005: Instance-Peers Endpoint
**Source:** ./specs/instance-lock.md (Section: "Endpoints")

`POST /instance-peers` returns a JSON array of canonical peer identifiers, sorted.

## $REQ_LOCK_006: Shutdown Endpoint
**Source:** ./specs/instance-lock.md (Section: "Endpoints")

`POST /shutdown` initiates a clean shutdown. Returns 200 immediately; the process performs cleanup (wait for copies, upload snapshots, delete lock files, close listener) and exits 0.

## $REQ_LOCK_007: Lock Cleanup on Shutdown
**Source:** ./specs/instance-lock.md (Section: "On Shutdown")

On clean shutdown (normal exit, SIGINT, SIGTERM, or `POST /shutdown`), `.kitchensync/lock` is deleted from every peer (best-effort -- connection failures are logged but do not prevent exit), and the TCP listener is closed.

## $REQ_LOCK_008: Race Condition Mitigation
**Source:** ./specs/instance-lock.md (Section: "Edge Cases")

After writing lock files, the instance re-reads all peers' lock files and verifies its own port is the one written. If another port appears, the instance with the lower port number wins; the other exits 1.

## $REQ_LOCK_009: Lock in Watch Mode
**Source:** ./specs/instance-lock.md (Section: "Watch Mode")

In `--watch` mode, the lock listener remains open for the duration of the session. Other instances attempting to sync any overlapping peer detect the running watcher and exit.

## $REQ_LOCK_010: Unreachable Peer During Lock Check
**Source:** ./specs/instance-lock.md (Section: "Edge Cases")

If a peer cannot be reached to read its `.kitchensync/lock` file, it is treated as clear. The sync will fail later for the same reason.

## $REQ_LOCK_011: Garbage Lock File Content
**Source:** ./specs/instance-lock.md (Section: "Edge Cases")

If a `.kitchensync/lock` file contains garbage (not a valid port number), it is treated as no lock -- the file is deleted and startup continues.
