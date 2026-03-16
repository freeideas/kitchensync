# Single Instance

How KitchenSync ensures only one instance manages a given sync root at a time.

## Why This Matters

Two instances managing the same sync root would concurrently modify manifests, tombstones, and XFER staging, corrupt change detection, and produce duplicate or conflicting sync operations.

## Mechanism

On startup, KitchenSync checks whether another instance is already managing this sync root. If so, it prints the existing port and exits (exit code 0, not an error). If not, it takes ownership.

This is handled by the quartz-lifecycle startup sequence (see `quartz-lifecycle.md`):

1. Init the database at `.kitchensync/kitchensync.sqlite`.
2. Read the `config` table for key `"serving-port"`. If found, `POST /app-path` on `http://127.0.0.1:{port}`. If the returned canonical path matches this app's canonical path, print the port number to stdout and `exit(0)`. If anything fails (no row, connection refused, path mismatch), continue.
3. Bind to `127.0.0.1:0` (OS-assigned ephemeral port), upsert `"serving-port"` in the `config` table with the assigned port, print the port number to stdout.

### Shutdown

The app is stopped via `POST /shutdown` (see `quartz-lifecycle.md`). If the process crashes, the OS releases the port and the `serving-port` row becomes stale — the next instance will detect this in step 2 (connection refused) and take over.

## API

The HTTP API listens on `127.0.0.1` only (not externally accessible). Endpoints are defined by quartz-lifecycle:

- `POST /app-path` — returns the canonical path to the running binary as a JSON string
- `POST /shutdown` — graceful shutdown (requires a current UTC timestamp in the body)

## Why Not a PID-Based Lockfile?

A traditional lockfile stores a PID. This is unreliable because:

1. **Stale locks after crashes.** The process dies without cleaning up; the PID in the lockfile no longer exists, or worse, has been reused by an unrelated process.
2. **PID reuse.** Operating systems recycle PIDs. A stale lockfile containing PID 12345 might match a completely unrelated process that was later assigned the same PID.

The port-based approach avoids both problems: if the process dies, the OS releases the port, and any connection attempt to it will fail. The `serving-port` row in the database becomes stale but harmless — the next instance's identity challenge (canonical path match via `POST /app-path`) will fail and take over.

## Why Port 0 Instead of a Fixed or Random Port?

1. **No collisions.** The OS guarantees the assigned port is currently unused.
2. **No configuration.** No need to choose a port range or handle "port already in use" retries.
3. **Firewall-friendly.** Ephemeral ports from the OS are typically in a range that localhost traffic can use without special rules.
