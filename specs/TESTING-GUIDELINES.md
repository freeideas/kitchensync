# Testing Guidelines

## Strategy

Tests use `file://` URLs and `sftp://` URLs. SFTP tests connect to `sftp://192.168.0.252/Volumes/Movx/tmp/` or `sftp://qube/Volumes/Movx/tmp/` (these are two different paths for the same directory) and subdirectories beneath it as peer roots. This directory is reserved for test use and may be created, populated, and cleaned up by tests freely.

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

Use `POST /shutdown` to the instance's lock port (read from `.kitchensync/lock`) to stop a running KitchenSync process. This triggers a clean shutdown (wait for in-progress copies, upload final snapshots, delete lock files) and is identical on every platform. See `specs/instance-lock.md` for details.

Never send signals directly via `os.kill()` or the `signal` module:

- **Windows:** `CTRL_C_EVENT` broadcasts to the entire console process group, killing the test runner along with the target process. `SIGTERM` does not exist.
- **Unix:** `os.kill(pid, signal.SIGTERM)` works but does not clean up process groups or child processes the way `subprocess` does.

If `POST /shutdown` is not possible (e.g. the lock file hasn't been written yet), use `process.terminate()` as a fallback -- it handles platform differences correctly (SIGTERM on Unix, `TerminateProcess` on Windows). Use `Popen.kill()` only as a last resort.

## Watch Mode Tests

Watch mode tests start a long-running `--watch` process, create/modify files, and verify that changes propagate. These tests are timing-sensitive:

- **Windows filesystem notification latency** is higher than Linux/macOS. After creating or modifying a file, allow at least 5 seconds for the watcher to detect the change and for the sync to complete. Shorter waits cause flaky failures on Windows even when the code is correct.
- Use the `WatchProcess.wait_for(pattern, timeout)` helper to wait for specific output (e.g., `watching`) before proceeding with file modifications. This avoids races where files are created before the watcher is ready.
- After modifying files, wait long enough for the event to be queued, processed, and the sync to complete before stopping the process and checking results.
- **File pre-aging for debounce tests**: Tests that exercise mod-time debounce behavior (e.g., REQ_WATCH_004's 5-second `TimeTolerance`) must pre-age files before bootstrapping the snapshot DB. Use `os.utime(path, (past, past))` to set the file's mtime at least 30 seconds in the past before the initial sync or DB seed. Without this, a subsequent write may produce an mtime identical to (or within tolerance of) the snapshot's recorded value, causing the debounce logic to suppress the event as a no-op — especially on filesystems with coarse timestamp resolution.
