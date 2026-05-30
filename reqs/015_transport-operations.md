# 015_transport-operations: Transport operation contract

## Behavior
This concern derives from `specs/sync.md` sections "Peer Transports", "Required Operations", "Error Semantics", and "Case Sensitivity". It covers the observable operation contract shared by `file://` and `sftp://` peers after connection, required directory, file, rename, delete, stat, stream, and mod-time operations, cross-transport error categories, omission of non-regular filesystem entries, and filename case preservation.

## $REQ_IDs
- `015.1` -- `file://` peers and `sftp://` peers provide the same transport operation behavior for equivalent filesystem state.
- `015.2` -- Directory listing returns only the immediate children of the requested directory.
- `015.3` -- Directory listing returns each listed child's name, `is_dir`, `mod_time`, and `byte_size`.
- `015.4` -- Directory listing reports a regular file's `byte_size` as its size in bytes.
- `015.5` -- Directory listing reports a directory's `byte_size` as `-1`.
- `015.6` -- Directory listing omits symbolic links.
- `015.7` -- Directory listing omits special files and other non-regular entry types.
- `015.8` -- Stat returns `mod_time`, `byte_size`, and `is_dir` for an existing regular file or directory.
- `015.9` -- Stat returns `not found` for a missing path.
- `015.10` -- Stat returns `not found` for a symbolic link, special file, or other non-regular entry type.
- `015.11` -- Streaming read opens a file and returns its bytes in chunks until EOF.
- `015.12` -- Closing a streaming read handle closes the read operation.
- `015.13` -- Streaming write creates the target file when it does not exist.
- `015.14` -- Streaming write creates missing parent directories for the target file.
- `015.15` -- Streaming write makes written bytes visible as the target file when the write handle is closed.
- `015.16` -- Rename moves an entry within the same filesystem when the destination does not already exist.
- `015.17` -- Rename does not overwrite an existing destination.
- `015.18` -- File delete removes the requested file.
- `015.19` -- Directory creation creates the requested directory and any needed parents.
- `015.20` -- Directory delete removes the requested empty directory.
- `015.21` -- Modification-time update sets the requested modification time on a file or directory.
- `015.22` -- Transport operations report failures using the normalized categories `not found`, `permission denied`, and `I/O error` regardless of transport.
- `015.23` -- SFTP connection drops and timeouts surface as `I/O error` transport failures.
- `015.24` -- Filenames are preserved exactly as the filesystem reports them.

## Notes
This category owns what an already-connected peer transport can do and how transport-level results are normalized. Startup reachability, fallback URL selection, root creation, SFTP authentication, and host key behavior belong to `004_peer-connectivity`; safe copy replacement sequencing belongs to `010_file-transfer-safety`; traversal-level listing concurrency and listing-error subtree consequences belong to `007_traversal-and-excludes`.
