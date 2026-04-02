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

## Stopping Child Processes

Use `process.terminate()` to stop child processes -- never send signals directly via `os.kill()` or the `signal` module. Platform-specific signal handling is full of gotchas:

- **Windows:** `CTRL_C_EVENT` broadcasts to the entire console process group, killing the test runner along with the target process. `SIGTERM` does not exist.
- **Unix:** `os.kill(pid, signal.SIGTERM)` works but does not clean up process groups or child processes the way `subprocess` does.

`Popen.terminate()` handles all of this correctly on every platform (SIGTERM on Unix, `TerminateProcess` on Windows). Use `Popen.kill()` only as a last resort if `terminate()` doesn't work within a reasonable timeout.

## Watch Mode Tests

Watch mode tests start a long-running `--watch` process, create/modify files, and verify that changes propagate. These tests are timing-sensitive:

- **Windows filesystem notification latency** is higher than Linux/macOS. After creating or modifying a file, allow at least 5 seconds for the watcher to detect the change and for the sync to complete. Shorter waits cause flaky failures on Windows even when the code is correct.
- Use the `WatchProcess.wait_for(pattern, timeout)` helper to wait for specific output (e.g., `watching`) before proceeding with file modifications. This avoids races where files are created before the watcher is ready.
- After modifying files, wait long enough for the event to be queued, processed, and the sync to complete before stopping the process and checking results.
