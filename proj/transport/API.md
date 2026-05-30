# Transport API

Rust module path: `kitchensync::transport`.

The `transport` module exports the connected filesystem primitive API used by
peer, snapshot, sync, operations, and runtime code. It hides whether a connected
peer is backed by a local `file://` root or an SSH/SFTP root.

## Root Contracts Consumed

The public transport API uses root-owned shared contracts rather than defining
parallel transport-local shapes:

- `PeerUrl`: one already-selected peer URL to connect.
- `RelPath`: a root-relative path for every filesystem operation.
- `EntryMeta`: reported entry name, `EntryKind`, modification time, and byte
  size.
- `EntryKind`: regular file or directory.
- `Timestamp`: shared UTC modification time value.
- `TransportError`: normalized failure category.
- `TransportRootMode`: whether connection construction must require an
  existing peer root or may create a missing root and parents.
- Run timeout settings from `RunConfig` or a root-owned timeout value.

Callers outside `transport` must not depend on local OS metadata, SSH session
types, SFTP handles, SFTP status codes, or platform error codes.

## Public Types

### `TransportFactory`

`TransportFactory` is the public constructor surface for connected transport
handles.

Required operation:

```rust
pub trait TransportFactory {
    fn connect(
        &self,
        url: &PeerUrl,
        timeouts: TransportTimeouts,
        root_mode: TransportRootMode,
    ) -> Result<TransportHandle, TransportError>;
}
```

`connect` accepts exactly one already-selected `PeerUrl`. URL fallback order,
peer roles, authentication ordering, and known host policy belong to the peer
module before this call. The peer module also chooses the `root_mode`: normal
runs use create-missing mode, and dry runs use require-existing mode. Transport
performs the root check or creation because it is the only layer with
scheme-specific access before a rooted `TransportHandle` exists.

### `TransportRootMode`

`TransportRootMode` is the public peer-root construction policy passed to
`TransportFactory::connect`.

Required shape:

```rust
pub enum TransportRootMode {
    RequireExisting,
    CreateMissing,
}
```

`RequireExisting` returns `TransportError::not_found` when the selected peer
root is absent. `CreateMissing` creates the selected peer root and any missing
parents before returning a handle. Permission failures and other creation
failures use the normal transport error categories.

### `TransportTimeouts`

`TransportTimeouts` is the public timeout record passed to connection
construction. It carries the connection timeout and idle keep-alive settings
needed by SFTP transports. Local transports may accept the value without using
all fields.

The record is owned by the root run configuration layer. The transport module
may re-export the root type or accept it directly, but it must not define a
separate timeout policy that callers must reconcile.

### `TransportHandle`

`TransportHandle` represents one connected peer root. All operations are
relative to that root and accept `RelPath` values.

Required operations:

```rust
impl TransportHandle {
    pub fn list_dir(&self, path: &RelPath) -> Result<Vec<EntryMeta>, TransportError>;
    pub fn stat(&self, path: &RelPath) -> Result<EntryMeta, TransportError>;

    pub fn open_read(&self, path: &RelPath) -> Result<TransportRead, TransportError>;
    pub fn open_write(&self, path: &RelPath) -> Result<TransportWrite, TransportError>;

    pub fn rename_no_overwrite(
        &self,
        src: &RelPath,
        dst: &RelPath,
    ) -> Result<(), TransportError>;

    pub fn delete_file(&self, path: &RelPath) -> Result<(), TransportError>;
    pub fn create_dir(&self, path: &RelPath) -> Result<(), TransportError>;
    pub fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError>;

    pub fn set_mod_time(
        &self,
        path: &RelPath,
        time: Timestamp,
    ) -> Result<(), TransportError>;
}
```

An implementation may expose these operations through an owned struct, trait
object, or enum-backed handle, but the public behavior above is the stable
contract other modules may rely on.

### `TransportRead`

`TransportRead` is the read stream returned by `TransportHandle::open_read`.
It must support Rust-native streaming reads without requiring the whole file to
be buffered by transport.

Required public behavior:

- implements `std::io::Read`, or an equivalent project trait whose read
  failures normalize to `TransportError`;
- reports EOF through normal Rust read semantics;
- releases the underlying local or SFTP file resource when closed or dropped.

If the concrete read type implements `std::io::Read`, read errors that cross
the transport boundary must still be convertible or reported as
`TransportError` categories before sibling modules make product decisions.

### `TransportWrite`

`TransportWrite` is the write stream returned by `TransportHandle::open_write`.
It must support Rust-native streaming writes without requiring the caller to
buffer the whole file.

Required public behavior:

- implements `std::io::Write`, or an equivalent project trait whose write and
  close failures normalize to `TransportError`;
- creates the destination file and any missing parent directories needed for
  that file during `open_write`;
- finalizes and flushes bytes so they are visible at the target path when the
  writer is explicitly closed or successfully dropped;
- releases the underlying local or SFTP file resource when closed or dropped.

The API should provide an explicit close/finalize operation when the concrete
writer type can fail during finalization. That close operation returns
`Result<(), TransportError>`.

## Operation Semantics

### Listing

`list_dir(path)` returns immediate children only. Each returned `EntryMeta`
preserves the filename exactly as reported by the backing filesystem.

Supported entry kinds:

- regular files, with byte size in bytes;
- directories, with byte size `-1`.

Symbolic links, devices, FIFOs, sockets, and any other unsupported entry types
are silently omitted from listing results for both local and SFTP transports.

### Stat

`stat(path)` returns metadata for an existing regular file or directory.
Missing paths, symbolic links, special files, and unsupported non-regular
entries return `TransportError::not_found`.

### Reading

`open_read(path)` opens an existing regular file for streaming reads. The
caller owns copy planning, retries, progress, diagnostics, and destination
sequencing around the stream.

### Writing

`open_write(path)` creates or truncates the destination file and creates any
missing parent directories needed for that file. Closing the writer finalizes
the content at the requested path. Transport does not create TMP, BAK, or SWAP
paths on its own.

### Rename

`rename_no_overwrite(src, dst)` performs a same-filesystem move that succeeds
only when `dst` does not already exist. Backends that would overwrite by
default must prevent overwrite and return a normalized error instead.

### Create and Delete

`create_dir(path)` creates the requested directory and any missing parents.
`delete_file(path)` deletes a regular file. `delete_dir(path)` deletes an empty
directory.

### Modification Time

`set_mod_time(path, time)` sets the modification time for a regular file or
directory using the shared `Timestamp` value.

## Errors

Every public transport operation returns either its success value or
`TransportError` in one of the root categories:

- `not_found`: the path is absent, or `stat` encounters a symlink, special
  file, or unsupported non-regular entry.
- `permission_denied`: the local filesystem or SFTP server denies access.
- `io_error`: every other failure, including local I/O failures, malformed
  remote responses, connection drops, SFTP channel failures, SSH keep-alive
  failures, and timeouts.

The category must be stable across local and SFTP transports for equivalent
conditions. Callers must not match on backend-specific diagnostics.

## Ownership and Lifetime Rules

- `TransportHandle` owns the connected backend session or root access state.
- `TransportRead` and `TransportWrite` own their open file resources.
- Dropping a handle or stream releases backend resources without requiring
  scheme-specific cleanup by sibling modules.
- Stream close/finalize failures that matter to correctness must be observable
  as `TransportError`.
- Transport handles are rooted at the selected peer root for their whole
  lifetime. Callers cannot retarget a handle to another root.
- Peer-root existence checks and creation happen during
  `TransportFactory::connect` according to `TransportRootMode`; operations on a
  returned handle remain root-relative.
- Thread-safety is part of the concrete Rust type contract: a handle may be
  `Send` or `Sync` only if the backend implementation can uphold that safely.

## Private Implementation Details

The following are not public API:

- local backend structs and host filesystem metadata;
- SSH session objects, SFTP client objects, file handles, channel errors, and
  protocol status codes;
- scheme dispatch internals;
- metadata filtering adapters;
- path conversion helpers below `RelPath`;
- retry, fallback, dry-run, progress, diagnostics, SWAP, BAK, TMP, snapshot, and
  sync decision behavior.

Other modules may rely only on the connected filesystem operations and
normalized error behavior documented here.
