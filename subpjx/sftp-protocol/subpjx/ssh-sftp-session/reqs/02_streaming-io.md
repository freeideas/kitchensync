# 02_streaming-io: open_read/read/close_read and open_write/write/close_write stream file bytes.

## Behavior
The streaming file-I/O operations (specs/SPEC.md §"Operations on a session") expose chunked read and write against a remote file. To read: `open_read` returns a handle; repeated `read(handle, max_bytes)` calls return up to `max_bytes` of content, signalling `EOF` once the file is exhausted; `close_read` releases the handle. To write: `open_write` returns a handle and creates the target file along with any missing parent directories; `write(handle, bytes)` appends a chunk; `close_write` finalizes the file. As with all session operations, failures are categorized as `not_found`, `permission_denied`, or `io_failure`.

## $REQ_IDs
- `02.19` — `open_read` on an existing file returns a handle that can be used to read its bytes.
- `02.20` — Repeated `read` calls on a read handle return up to `max_bytes` each and together yield the file's full content.
- `02.21` — `read` signals `EOF` once the file has been fully consumed.
- `02.22` — `open_read` returns `not_found` for a missing path.
- `02.23` — A file created via `open_write` / `write` / `close_write` exists at the given path and contains the concatenation of the written byte chunks.
- `02.24` — `open_write` creates missing parent directories along the path.

## Notes
`close_read` and `close_write` are end-of-handle hooks; their observable effect is that the file is present and readable (write side) or that subsequent operations behave normally (read side). No bullet asserts internal handle release directly — outcome bullets cover the testable surface.
