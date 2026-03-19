# Testing Guidelines

## Strategy

All automated tests use `file://` URLs exclusively. SFTP is verified manually.

This works because all sync logic operates through the peer filesystem trait (see sync.md, "Peer Filesystem Abstraction"). The `file://` and `sftp://` implementations expose identical operations with identical error semantics. No sync logic touches protocol-specific code. Testing with `file://` exercises every code path that `sftp://` will hit — the only untested surface is the SSH library itself.

## What Tests Should Cover

Using `file://` URLs and temporary directories:

- **Multi-tree traversal** — parallel listing, union, correct decisions across N peers
- **Decision rules** — timestamp-based (newer wins, ties keep data), `--canon` override
- **Tombstones** — deletion propagation, resurrection, expiry
- **Snapshot** — updated during traversal, discrepancies detected on next run
- **XFER staging** — atomic swaps, recheck, cleanup of stale dirs
- **BACK directories** — displaced files recoverable, retention cleanup
- **Config resolution** — all three forms (file, .kitchensync/ dir, parent dir)
- **Offline peers** — skipped gracefully, caught up on next run
- **Single instance** — second run detects first and exits
- **Edge cases** — empty directories, deep paths, timestamp tolerance
- **Abstraction boundary** — every test interacts with peers only through the trait; no test bypasses it to manipulate files directly after setup

## Test Structure

Tests are Python scripts in `./tests/`. Each test:

1. Creates temporary directories for simulated peers
2. Creates `kitchensync-conf.json` with `file://` URLs
3. Sets up initial file states
4. Runs `kitchensync <config>`
5. Verifies outcomes (files synced, snapshot correct, BACK/ contents)
6. Cleans up

Tests should be independent and not rely on execution order.
