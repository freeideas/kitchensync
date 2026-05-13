# 03_error-categories: Three-category error surface for all operations

## Behavior

Every operation surfaces failures in exactly three categories, regardless of the underlying SSH/SFTP status code: "not found" for missing paths, "permission denied" for authorization refusals, and "I/O error" for everything else. Network and transport-level failures (connection reset, handshake timeout, SFTP channel death, protocol error, remote write failure, server-side error) all collapse into the "I/O error" category — callers do not distinguish transport from on-disk failures. Derives from `specs/SPEC.md` § "API surface > Error categories".

## $REQ_IDs

- `03.1` — Operations against a non-existent path surface a "not found" error.
- `03.2` — Operations the remote refuses for authorization reasons surface a "permission denied" error.
- `03.3` — A failed SSH handshake (including `connect_timeout_seconds` expiry) surfaces as an "I/O error" on the affected operation.
- `03.4` — A network failure during an in-progress operation (connection reset, SFTP channel death) surfaces as an "I/O error".
- `03.5` — Server-side or protocol-level failures not classified as "not found" or "permission denied" surface as an "I/O error".

## Notes

- See [[02_pool-acquire-release]] for the handshake timeout parameter and [[02_host-key-verification]] for connection rejections — both surface here as I/O errors.
