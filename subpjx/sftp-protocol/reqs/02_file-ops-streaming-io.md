# 02_file-ops-streaming-io: Streaming reads and writes

## Behavior

A connection handle exposes streaming read and write operations. `open_read(path)` returns a read handle; chunks are pulled via `read(handle, max_bytes)` until EOF; `close_read(handle)` releases the handle. `open_write(path)` returns a write handle, creating the file if absent and creating any missing parent directories along the way; chunks are pushed via `write(handle, bytes)`; `close_write(handle)` flushes and closes. Derives from `specs/SPEC.md` § "API surface > File operations".

## $REQ_IDs

- `02.26` — `open_read(path)` opens a regular file for streaming reads and returns a read handle.
- `02.27` — `read(handle, max_bytes)` returns the next chunk of bytes from the read handle.
- `02.28` — `read(handle, max_bytes)` reports EOF after the last chunk has been returned.
- `02.29` — `close_read(handle)` closes a read handle.
- `02.30` — `open_write(path)` opens a regular file for streaming writes and returns a write handle.
- `02.31` — `open_write(path)` creates the target file if it does not already exist.
- `02.32` — `open_write(path)` creates any missing parent directories along `path`.
- `02.33` — `write(handle, bytes)` appends the next chunk to the write handle.
- `02.34` — `close_write(handle)` flushes the write handle and closes it; the bytes written are observable in the file after close.

## Notes

- Paths are absolute and use forward-slash separators.
