# Testing Guidelines

## Strategy

Tests use `file://` URLs and `sftp://` URLs. SFTP tests connect to localhost as the current user, using `sftp://ace@localhost/home/ace/Desktop/prjx/kitchensync/tmp/testks/` and subdirectories beneath it as peer roots. This directory is reserved for test use and may be created, populated, and cleaned up by tests freely.

## What Tests Should Cover

Using `file://` URLs (with temporary directories) and `sftp://` URLs (with subdirectories under `/home/ace/Desktop/prjx/kitchensync/tmp/testks/`):

- **Multi-tree traversal** — parallel listing, union, correct decisions across N peers
- **Decision rules** — timestamp-based (newer wins, ties keep data), canon peer override
- **Peer groups** — group formation from CLI URLs, group recognition from a single URL, accumulation in config file
- **Canon peer** — required on first sync (no snapshot), optional after snapshots exist
- **Tombstones** — deletion propagation, resurrection, expiry
- **Snapshot** — updated during traversal, discrepancies detected on next run
- **Peer identity** — URL normalization, peer recognition across runs, fallback URLs sharing a peer ID, startup reconciliation (two-pass)
- **XFER staging** — atomic swaps, recheck, cleanup of stale dirs
- **BACK directories** — displaced files recoverable, retention cleanup
- **Config directory** — default `~/.kitchensync/`, `--cfgdir` override, config file accumulation
- **Offline peers** — skipped gracefully, caught up on next run
- **Single instance** — second run against same config directory detects first and exits
- **Connection pools** — per-URL pools, `max-connections` limit applies to `file://` too
- **Edge cases** — empty directories, deep paths, timestamp tolerance (5 seconds)
- **Abstraction boundary** — every test interacts with peers only through the trait; no test bypasses it to manipulate files directly after setup

## Test Structure

Tests are Python scripts in `./tests/`. Each test:

1. Creates temporary directories for simulated peers
2. Creates a config directory with `kitchensync-conf.json` containing `file://` URLs grouped appropriately
3. Sets up initial file states
4. Runs `kitchensync --cfgdir <config-dir> <url>...` or `kitchensync --cfgdir <config-dir>` (if config already has groups)
5. Verifies outcomes (files synced, snapshot correct, BACK/ contents)
6. Cleans up

Tests should be independent and not rely on execution order.
