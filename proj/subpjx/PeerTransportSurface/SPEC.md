# PeerTransportSurface:

## Purpose

PeerTransportSurface defines the common peer operation surface that sync logic
uses after startup has selected a reachable URL for each peer. Local filesystem
peers and SFTP peers must expose the same operations, entry shape, path
semantics, and error categories through this surface.

Every operation is scoped to the already connected peer root for that peer's
winning URL. Callers pass paths relative to that root. The surface does not
retry fallback URLs after startup, and it does not expose transport-specific
paths, sessions, handles, or error types to sync logic.

## Responsibilities

The surface exposes these operations for connected peers:

- `list_dir(peer, path)` lists exactly the immediate children of `path`. It
  does not recurse. Each returned entry contains the child name, `is_dir`,
  `mod_time`, and `byte_size`.
- `stat(peer, path)` returns `mod_time`, `byte_size`, and `is_dir` for a
  regular file or directory.
- `open_read(peer, path)`, `read(handle, max_bytes)`, and
  `close_read(handle)` stream file bytes from a peer file. Each read returns
  the next byte chunk or EOF.
- `open_write(peer, path)`, `write(handle, bytes)`, and
  `close_write(handle)` stream file bytes to a peer file. Opening a writer
  creates the target file and any needed parent directories. Closing the
  writer finalizes the file so later peer reads return the written bytes.
- `rename(peer, src, dst)` moves `src` to a non-existing `dst` on the same
  filesystem.
- `delete_file(peer, path)` removes a file.
- `create_dir(peer, path)` creates a directory and any needed parent
  directories.
- `delete_dir(peer, path)` removes an empty directory.
- `set_mod_time(peer, path, time)` sets the modification time of an existing
  file or directory.

Directory entries and stat results use one shared metadata shape:

- `child name` is the name exactly as the peer filesystem reports it. The
  surface must not change case, normalize Unicode, rewrite separators inside
  the name, or otherwise canonicalize reported filenames.
- `is_dir` is true only for directories and false for regular files.
- `mod_time` is the peer modification time value used by the rest of the
  product for comparison, snapshot storage, and copy preservation.
- `byte_size` is the file size in bytes for regular files.
- `byte_size` is `-1` for directories.

`list_dir` omits symbolic links, special files, device files, FIFOs, sockets,
and every other non-regular entry type. `stat` reports `not found` for a
missing path and also reports `not found` for symbolic links, special files,
and every other non-regular entry type.

All operation failures crossing this boundary use the same categories for
local filesystem and SFTP peers:

- `not found` means the requested path is missing or is a non-regular entry
  type that the sync engine must treat as absent.
- `permission denied` means the peer rejected access because the user lacks
  permission for the requested operation.
- `I/O error` means any other operation failure, including transport I/O
  failures that cannot be reported as `not found` or `permission denied`.

## Boundaries

PeerTransportSurface does not decide which peer URL wins startup connection,
does not create missing peer roots during startup, does not authenticate SFTP
sessions, and does not own SSH host-key checking or timeout handling.

PeerTransportSurface does not own sync decisions, listing retry policy, global
copy-slot limits, progress output, dry-run suppression of peer writes,
recoverable SWAP and BAK staging, or snapshot database updates. Those callers
choose when operations are allowed and which paths to pass.

The surface only guarantees `rename` into a destination that does not already
exist. Callers must use staging flows when replacing existing files or
snapshot databases and must not depend on transport-specific overwrite
behavior.

Read and write handles are operation-scoped resources. After a handle is
closed, later reads or writes through that handle are outside this surface's
guarantees. A closed write handle must leave a finalized file or return a
failure category explaining why finalization failed.
