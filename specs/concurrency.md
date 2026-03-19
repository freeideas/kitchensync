# Concurrency

## Per-Peer Limits

Each peer maintains independent concurrency limits:

| Limit          | Default | Config key   |
| -------------- | ------- | ------------ |
| Max reads      | 10      | `max-reads`  |
| Max writes     | 10      | `max-writes` |

These are global defaults in the config root. A file transfer from peer A to peer B consumes one read slot on A and one write slot on B for the duration of the transfer. Both slots must be available before the transfer begins. When the transfer completes (or fails), both slots are released.

If either slot is unavailable, the transfer waits until a slot frees up.

## Parallel Directory Listing

During multi-tree traversal, directory listings for all reachable peers at each level must be issued concurrently, not sequentially. With N reachable peers, the wall-clock time for listing one directory level should be approximately the time of the slowest peer, not the sum of all peers.

## Trace Logging

When log level is `trace`, log every slot count change:

```
peer=<name> reads=<n>/<max> writes=<n>/<max>
```

Logged on every acquire and release. This allows tests to reconstruct the concurrency timeline from the `applog` table and verify that limits were never exceeded.

## Testing

### Concurrency limit test

1. Create 3+ peers
2. Populate enough files (50+) that transfers would naturally exceed per-peer limits
3. Set log level to `trace`
4. Run sync
5. Query `applog` for slot count log entries
6. Assert no peer ever exceeded its configured `max-reads` or `max-writes`

### Parallel directory listing test

This is a code examination test, not a runtime test. The test reads the source files that implement the multi-tree traversal and verifies that directory listings across peers are issued concurrently. Specifically, it must confirm that the code does not list peers sequentially (e.g., awaiting each peer's listing before starting the next). Look for patterns such as: all peer listings collected into a concurrent join/gather/parallel construct rather than a sequential loop with individual awaits.
