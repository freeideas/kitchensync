# 03_parallel-listing: Directory listings across peers are concurrent

## Behavior

When the multi-tree walk reaches a directory, it issues `list_dir` against every reachable peer concurrently, not one peer at a time. With N reachable peers, the wall-clock time for listing one directory level should be near the slowest peer rather than the sum of all peers. Derived from `concurrency.md` §"Directory Listing" / §"Parallel directory listing test".

## $REQ_IDs

- `03.75` — The source code that implements the multi-tree walk issues per-level peer listings via a concurrent join/gather/parallel construct.
- `03.76` — The source code does not list peers in a sequential loop that awaits each peer's listing before starting the next.
- `03.77` — Directory listing uses its own connection per peer, outside the file-transfer pool.

## Notes

This is a code-examination requirement: tests verify the structure of the traversal implementation. The pool semantics for transfer connections live in `03_sftp-pool.md`.
