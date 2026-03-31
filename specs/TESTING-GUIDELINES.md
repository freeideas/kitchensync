# Testing Guidelines

## Strategy

Tests use `file://` URLs and `sftp://` URLs. SFTP tests connect to `sftp://ace@contabix/tmp/kstest/` and subdirectories beneath it as peer roots. This directory is reserved for test use and may be created, populated, and cleaned up by tests freely.

## What Tests Should Cover

Using `file://` URLs (with temporary directories) and `sftp://` URLs (with subdirectories under the test root):

- **Multi-tree traversal** — parallel listing, union, correct decisions across N peers
- **Decision rules** — timestamp-based (newer wins, ties keep data), canon peer (`+`) override
- **Subordinate peers** — `-` peers don't influence decisions, receive group outcome, pre-existing files displaced
- **Canon peer (`+`)** — required on first sync (no snapshot), optional after snapshots exist
- **Tombstones** — deletion propagation, resurrection, expiry
- **Snapshot** — stored per-peer in `.kitchensync/snapshot.db`, downloaded/uploaded atomically, discrepancies detected on next run
- **Fallback URLs** — bracket syntax `[url1,url2]`, first that connects wins, shared snapshot
- **Per-URL settings** — query-string `?mc=5&ct=60` overrides global `--mc`/`--ct`
- **TMP staging** — atomic swaps, recheck, cleanup of stale dirs
- **BAK directories** — displaced files recoverable, retention cleanup
- **Offline peers** — skipped gracefully, caught up on next run
- **Connection pools** — per-URL pools, `--mc` limit applies to `file://` too
- **Edge cases** — empty directories, deep paths, timestamp tolerance (5 seconds)
- **Abstraction boundary** — every test interacts with peers only through the interface; no test bypasses it to manipulate files directly after setup
- **Snapshot atomicity** — concurrent runs don't corrupt snapshot (atomic rename pattern)

## Test Structure

Tests are Python scripts in `./tests/`. Each test:

1. Creates temporary directories for simulated peers
2. Sets up initial file states (and optionally pre-populates `.kitchensync/snapshot.db` for peers with history)
3. Runs `kitchensync <peer> <peer> [options]` with appropriate `+`/`-` prefixes
4. Verifies outcomes (files synced, snapshot correct, BAK/ contents)
5. Cleans up

Tests should be independent and not rely on execution order.
