# Testing Guidelines

How automated tests should approach KitchenSync.

## Strategy

All automated tests use `file://` URLs exclusively. SFTP functionality is not tested automatically -- it is verified manually.

## Why Not Test SFTP?

1. **Infrastructure burden.** Automated SFTP testing requires spinning up an SSH server, managing keys, and handling platform differences. This complexity outweighs the benefit.

2. **Thin layer.** The SFTP implementation is a thin wrapper over a well-tested SSH library. The interesting logic (reconciliation, conflict resolution, queues, walks, staging) is transport-agnostic.

3. **Same code path.** Both `file://` and `sftp://` use the same peer filesystem abstraction (see `peers.md`). Testing with `file://` exercises the same sync logic that SFTP uses.

## What Tests Should Cover

Using `file://` URLs and temporary directories:

- **Local walker** -- detects new, modified, and deleted files; updates database; enqueues to peers
- **Peer walker** -- detects differences between local and peer; enqueues appropriately
- **Reconciliation** -- conflict resolution (newer wins, ties favor keeping data), push/pull decisions
- **Tombstones** -- deletion propagation, resurrection handling, 6-month expiry
- **XFER staging** -- atomic swaps, cleanup of incomplete transfers
- **BACK directories** -- displaced files recoverable, 90-day cleanup
- **Queue behavior** -- deduplication, overflow handling, persistence across runs
- **Ignore rules** -- `.syncignore` parsing, pattern matching, hierarchy
- **Timestamps** -- correct format, comparison logic
- **Once mode vs watch mode** -- different behaviors tested appropriately
- **Single instance** -- second instance detects first and exits
- **Edge cases** -- empty directories, deep paths, special characters in filenames

## Ensuring SFTP Will Work

Even though SFTP isn't tested automatically, the code should be structured to maximize confidence:

1. **Abstraction boundary.** The peer filesystem trait must be the only place where `file://` and `sftp://` differ. No transport-specific logic in sync code.

2. **Error handling.** SFTP operations can fail in ways local filesystem operations won't (network timeout, connection drop, permission denied remotely). The abstraction should surface these as errors the sync logic handles gracefully.

3. **Logging.** SFTP operations should log enough detail that manual testing can diagnose issues without code changes.

4. **Manual test checklist.** Before release, manually verify:
   - Connect to real SFTP server
   - Push a file, pull a file
   - Handle connection drop mid-transfer
   - Verify BACK/ displacement works on remote
   - Test with slow/high-latency connection

## Test Structure

Tests are Python scripts in `./tests/`. Each test:

1. Creates temporary directories for local sync root and simulated peers
2. Sets up `.kitchensync/peers.conf` with `file://` URLs
3. Creates initial file states
4. Runs `kitchensync --once`
5. Verifies expected outcomes (files synced, databases updated, BACK/ contents)
6. Cleans up

Tests should be independent and not rely on execution order.
