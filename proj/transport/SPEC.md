# transport:

## Purpose

Own the connected filesystem operation boundary shared by local `file://` peers and SSH/SFTP peers. The module presents both schemes through one Rust transport contract so callers can list, stat, stream, create, rename, delete, and set modification times without knowing which scheme backs a reachable peer.

The transport module is responsible for observable parity between local filesystem and SFTP operations after a peer URL has been selected for the run. It preserves filenames exactly as reported by the backing filesystem, hides unsupported entry types from sync-visible metadata, and converts implementation-specific failures into the root `TransportError` categories: `not_found`, `permission_denied`, and `io_error`.

## Responsibilities

- Provide a `TransportFactory` or equivalent constructor surface for the peer module to obtain a connected `TransportHandle` for a selected `file://` or `sftp://` URL, using the requested peer-root construction mode.
- During connection construction, require the selected peer root to exist or create the missing root and parents according to the `TransportRootMode` chosen by `peer`.
- Treat a connected handle as rooted at the selected peer root. All operation paths are root-relative `RelPath` values or module-internal metadata paths derived from them.
- Implement `list_dir(path)` for immediate children only. Each returned child must include the reported name, entry kind, modification time, and byte size. Regular files report their byte size in bytes; directories report byte size `-1`.
- Omit symbolic links, devices, FIFOs, sockets, and other non-regular entry types from `list_dir` results. The omission is silent and must be consistent across local and SFTP transports.
- Implement `stat(path)` for existing regular files and directories, returning modification time, byte size, and entry kind. Missing paths, symbolic links, special files, and other non-regular entry types must return `TransportError::not_found`.
- Implement streaming reads with `open_read(path)`, chunk reads, EOF reporting, and `close_read(handle)`. The module must not require callers to buffer an entire file before bytes can be read.
- Implement streaming writes with `open_write(path)`, chunk writes, and `close_write(handle)`. Opening a write must create the destination file and any missing parent directories needed for that file. Closing a write must flush/finalize the file so the written bytes are visible at the target path.
- Implement `rename(src, dst)` as a same-filesystem move that succeeds only when `dst` does not already exist. If a host API would overwrite by default, the transport implementation must prevent overwrite and report a normalized error instead.
- Implement `delete_file(path)` for regular files, `create_dir(path)` for a directory plus any missing parents, and `delete_dir(path)` for an empty directory.
- Implement `set_mod_time(path, time)` for files and directories using the shared `Timestamp` value supplied by callers.
- Preserve filename spelling and case exactly as the filesystem reports it. The transport module must not fold case, normalize names for decision-making, or hide case collisions.
- Apply SFTP connection timeout and idle keep-alive settings supplied by the peer module when constructing SFTP handles. SFTP connection drops, channel failures, and timeouts after connection must surface as `TransportError::io_error`.
- Map local OS errors and SFTP protocol/library errors into only the root categories `not_found`, `permission_denied`, or `io_error`. Callers outside this module must never need to match on platform error codes, SSH errors, SFTP status codes, or library-specific variants.
- Close or release transport read/write/session resources when handles are closed or dropped, without requiring sync, snapshot, operations, or runtime code to know scheme-specific cleanup details.

## Boundaries

- The CLI module owns parsing peer operands, URL syntax validation, global option validation, and help output.
- The peer module owns fallback URL order, startup reachability, root creation policy selection for normal versus dry-run mode, canon/subordinate role application, selected URL identity, SFTP authentication order, and known-host verification policy.
- The snapshot module owns SQLite snapshot files, local temporary database copies, snapshot SWAP recovery, snapshot upload sequencing, path hashing, and timestamp generation.
- The sync module owns traversal, retrying failed directory listings up to `--retries-list`, entry union construction, excludes, sync decisions, and listing-failure subtree consequences.
- The operations module owns safe replacement sequencing, SWAP/BAK/TMP path construction, displacement, cleanup, dry-run suppression of peer mutations, copy retry failure phases, and any local-to-local copy optimization. Transport only supplies the primitive operations those sequences call.
- The runtime module owns copy scheduling, active-copy limits, progress events, verbosity filtering, diagnostics, and stdout rendering.
- Transport does not decide whether an operation should be attempted. It performs a requested primitive operation or returns a normalized transport error.
- Transport does not implement command-line dry-run policy. Peer passes require-existing root mode for dry-run startup. After connection, callers avoid mutation operations in dry-run; if a caller does invoke a mutation method, transport treats it as a real primitive operation.
- Transport does not classify files as modified, deleted, new, canonical, subordinate, excluded, or conflicting. It reports live filesystem metadata only.

## Error Obligations

Every public transport operation returns either its specified success value or `TransportError` in one of these categories:

- `not_found`: the requested path is absent, or `stat` encounters a symlink, special file, or unsupported non-regular entry.
- `permission_denied`: the backing filesystem or SFTP server denies access because of permissions or authorization.
- `io_error`: all other operation failures, including local I/O failures, malformed remote responses, connection drops, SFTP channel failures, SSH keep-alive failures, and timeouts.

The category must be stable across schemes for equivalent conditions. For example, a missing file must be `not_found` for both `file://` and `sftp://`; a destination path that already exists during `rename(src, dst)` must not be treated as a successful overwrite on either scheme; and an SFTP timeout must not leak a scheme-specific timeout type to callers.

If an operation partially succeeds before failing, transport reports the failure category and leaves recovery decisions to the caller. Transport must not perform SWAP recovery, archive old files, update snapshots, retry copy work, or emit diagnostics on its own.
