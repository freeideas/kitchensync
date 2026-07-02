# TransportOperations:

## Purpose

TransportOperations provides the uniform peer filesystem surface used after a
peer has already been connected. It hides whether the winning peer URL is a
local `file://` root or an `sftp://` root and exposes the same operations,
metadata shape, streaming handles, and error categories to its callers.

The caller supplies a connected local or SFTP peer handle and paths relative to
that peer's sync root. This child performs the requested filesystem operation
against the peer's winning URL only. It does not choose fallback URLs, reconnect
to another URL, or decide sync outcomes.

## Responsibilities

TransportOperations exposes `list_dir(path)`. The operation lists only the
immediate children of `path`. For each regular file child it returns the child
name, modification time, byte size, and a non-directory entry type. For each
directory child it returns the child name, modification time, byte size `-1`,
and a directory entry type. It omits symbolic links. It also omits devices,
FIFOs, sockets, and any other entry that is neither a regular file nor a
directory.

TransportOperations exposes `stat(path)`. For an existing regular file or
directory, it returns modification time, byte size, and directory type using the
same metadata rules as `list_dir`. For a missing path, symbolic link, device,
FIFO, socket, or other non-regular non-directory entry, it returns the
`not_found` transport error category.

TransportOperations exposes streaming read operations:

- `open_read(path)` opens an existing regular file for streaming reads.
- `read(handle, max_bytes)` returns up to `max_bytes` of the next file-content
  bytes from that open read handle.
- `read(handle, max_bytes)` returns EOF after all file content has been
  returned.
- `close_read(handle)` closes the open read handle.

TransportOperations exposes streaming write operations:

- `open_write(path)` opens a file for streaming writes.
- `open_write(path)` creates the target file when it does not exist.
- `open_write(path)` creates missing parent directories for the target file.
- `write(handle, bytes)` writes the given bytes to the open write handle.
- `close_write(handle)` flushes and closes the open write handle.

TransportOperations exposes peer-mutating filesystem operations:

- `rename(src, dst)` moves an entry within the same peer filesystem when `dst`
  does not already exist.
- `delete_file(path)` removes a file.
- `create_dir(path)` creates a directory and any missing parent directories.
- `delete_dir(path)` removes an empty directory.
- `set_mod_time(path, time)` updates the modification time of a regular file.
- `set_mod_time(path, time)` updates the modification time of a directory.

For `file://` peers, every operation uses the local filesystem rooted at the
connected peer root. For `sftp://` peers, every operation uses the established
SSH/SFTP connection for that peer. Network failures that occur during SFTP
transport operations are reported as `io_error`.

All failures that cross this boundary use one of three transport error
categories: `not_found`, `permission_denied`, or `io_error`. The same category
must mean the same thing to callers for local and SFTP peers, so higher-level
sync behavior can depend on categories rather than transport scheme. For
example, a `not_found` from `stat` is the same absence signal for a local peer
and an SFTP peer, and an SFTP network failure is an `io_error` rather than a
separate network-specific outcome.

## Boundaries

TransportOperations does not parse command-line arguments, normalize peer URLs,
select fallback URLs, perform SSH host-key checks, authenticate credentials,
create peer roots during startup, or decide whether a peer is reachable. It
receives only connected peer handles from the startup connection owner.

TransportOperations does not decide which paths to copy, delete, displace,
archive, exclude, or recurse into. It does not own copy queue retries, global
copy slot limits, progress output, dry-run planning, BAK/TMP/SWAP policy,
snapshot rows, or database uploads. Other children call this child to perform
the concrete filesystem steps after they have made those decisions.

TransportOperations does not require `rename(src, dst)` to overwrite an
existing `dst`. Callers that replace data must use a safe sequence that works
when destination overwrite is rejected.

The invariant for all operations is transport parity: a caller can use the same
operation names, metadata fields, handle behavior, and error categories for
`file://` and `sftp://` peers. Paths are interpreted inside the already
connected peer root, and operations must not escape that root by following
symbolic links because symbolic links are treated as absent or omitted.
