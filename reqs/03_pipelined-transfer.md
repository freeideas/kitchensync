# 03_pipelined-transfer: Streaming file transfers are pipelined

## Behavior

Each file transfer is implemented as two concurrent tasks — one reading chunks from the source peer and one writing chunks to the destination peer — connected by a bounded channel. The reader and writer run simultaneously; a single-loop read-then-write pattern is not acceptable. Derived from `sync.md` §"File Copy" and `concurrency.md` §"Pipelined transfer test".

## $REQ_IDs

- `03.71` — The source code that implements file transfers uses two concurrent tasks (or futures): one for reading chunks from the source, one for writing chunks to the destination.
- `03.72` — The two transfer tasks are connected by a bounded channel that provides backpressure (reader blocks when full, writer blocks when empty).
- `03.73` — The transfer code does not contain a single loop that alternates between reading a chunk and writing it sequentially.
- `03.74` — Content is streamed chunk by chunk through the channel rather than buffering the entire file in memory.

## Notes

This is a code-examination requirement: tests verify the structure of the transfer implementation, not just observable I/O. The chunk-level read/write primitives are defined per transport in `sync.md` §"Required Operations".
