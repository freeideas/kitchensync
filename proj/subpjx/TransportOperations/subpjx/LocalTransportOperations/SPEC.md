# LocalTransportOperations:

## Purpose

LocalTransportOperations performs the `file://` side of TransportOperations
after the peer has already been connected. It receives a connected local peer
root and peer-relative paths, resolves each operation inside that root, and uses
the local filesystem to perform the requested filesystem step.

This child exists so the parent facade can keep one operation surface and one
set of transport error categories while the local backend stays small and
direct. It does not decide whether a peer is local, choose fallback URLs, or
define behavior for `sftp://` peers.

## Responsibilities

LocalTransportOperations exposes the local implementation of the parent
transport operations:

- `list_dir(path)` lists only the immediate children of a directory under the
  connected local root.
- `stat(path)` reports metadata for an existing regular file or directory.
- `open_read(path)`, `read(handle, max_bytes)`, and `close_read(handle)` stream
  bytes from an existing regular file.
- `open_write(path)`, `write(handle, bytes)`, and `close_write(handle)` stream
  bytes to a local file, creating the target file and any missing parent
  directories.
- `rename(src, dst)` moves an entry within the same local filesystem when
  `dst` does not already exist.
- `delete_file(path)` removes a local file.
- `create_dir(path)` creates a local directory and any missing parent
  directories.
- `delete_dir(path)` removes an empty local directory.
- `set_mod_time(path, time)` updates the modification time of a local regular
  file or directory.

For `list_dir(path)`, each regular file child is reported with its child name,
modification time, byte size, and non-directory type. Each directory child is
reported with its child name, modification time, byte size `-1`, and directory
type. Symbolic links are omitted. Devices, FIFOs, sockets, and other entries
that are neither regular files nor directories are omitted.

For `stat(path)`, a regular file or directory returns the same metadata shape
as `list_dir(path)`. A missing path, symbolic link, device, FIFO, socket, or
other non-regular non-directory entry returns the transport `not_found`
category.

Read handles and write handles are local file handles owned by this child.
Reading returns up to the requested byte count from the current handle position
and returns EOF after all file content has been returned. Closing a write handle
flushes pending local file data before the handle is released.

All errors that cross this boundary must use the parent transport categories:
`not_found`, `permission_denied`, or `io_error`. Missing local paths and local
entries treated as absent map to `not_found`. Local access-denied failures map
to `permission_denied`. Other local filesystem failures map to `io_error`.

## Boundaries

LocalTransportOperations does not parse or normalize peer URLs, create the peer
root during connection, authenticate remote peers, open SFTP sessions, or
translate SFTP errors. It receives only a connected local peer root from its
caller.

LocalTransportOperations does not choose sync actions, recurse through trees on
its own, manage copy queues, enforce copy slot limits, perform dry-run policy,
write snapshot rows, upload databases, or apply BAK/TMP/SWAP sequencing.
Callers decide what operation is needed and call this child for the concrete
local filesystem step.

The invariant for every operation is root containment. Peer-relative paths are
interpreted under the connected local root, and operations must not escape that
root. Symbolic links are never followed as peer entries: listing omits them, and
statting them reports `not_found`.

The second invariant is local parity with the parent transport surface. The
operation names, metadata fields, handle behavior, and error categories used by
this child must match the parent facade so callers can handle local and SFTP
peers through the same TransportOperations behavior.
