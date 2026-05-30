# Transport Architecture

The `transport` module owns the connected filesystem operation boundary shared
by local `file://` peers and SSH/SFTP peers. It exposes matching primitive file
tree operations for already-selected peer URLs, so higher modules can list,
stat, stream, create, delete, rename, and set modification times without
depending on backend-specific APIs or error values.

The module is intentionally not product-aware. It does not parse CLI peer
operands, choose fallback URLs, assign peer roles, make sync decisions,
sequence safe user-file replacement, retry copy work, or render diagnostics.

## Responsibilities

- construct a connected local or SSH/SFTP transport handle for one supported
  peer URL using the root construction mode selected by `peer`;
- list immediate directory entries while preserving reported filenames exactly;
- omit unsupported entry types from listings instead of exposing them through
  the shared entry model;
- stat files and directories without leaking backend metadata types;
- open streaming readers and writers for file content transfer;
- create files or directories as requested by callers;
- delete files or directories as requested by callers;
- rename without overwriting on both supported schemes;
- set modification times after writes;
- normalize local filesystem, SSH, SFTP, protocol, timeout, and permission
  failures into the root transport error categories.

## Public Surface

The public API should stay small and behavioral:

- `TransportHandle` represents one connected peer root and exposes `list_dir`,
  `stat`, `open_read`, `open_write`, `rename_no_overwrite`, create, delete, and
  `set_mod_time` operations.
- Connection construction accepts a single already-selected `PeerUrl`,
  `TransportRootMode`, and the SFTP timeout and keep-alive settings required to
  establish and operate the backend connection.
- Listing and stat results use the root `EntryMeta`, `EntryKind`, `RelPath`,
  and `Timestamp` contracts.
- All public failures use the root `TransportError` categories:
  `not_found`, `permission_denied`, and `io_error`.

Backend handles, native metadata, SSH session objects, SFTP status codes, OS
error codes, timeout values, and protocol-specific diagnostics are private
implementation details. Callers must match behavior through the root contracts
only.

## Internal Design

The module can be implemented as private files or components behind the single
public transport boundary:

- a scheme dispatcher that selects the local or SSH/SFTP backend for one
  supported `PeerUrl`;
- a root preparation step that requires the selected peer root to exist or
  creates missing root parents according to `TransportRootMode`;
- a local backend implemented over host filesystem APIs;
- an SSH/SFTP backend implemented over SSH session and SFTP file APIs;
- a metadata adapter that converts backend listing and stat results into root
  entry metadata while preserving names, reporting directory size as `-1`, and
  filtering unsupported kinds;
- a stream adapter that exposes backend reads and writes through the language
  native read/write traits expected by callers and finalizes resources on close
  or drop;
- a mutation adapter that implements create, delete, rename-without-overwrite,
  and modification-time operations with matching observable semantics;
- an error adapter that maps backend failures into the root transport error
  categories.

These components are private implementation structure. They should not create
new sibling-visible APIs or product behavior.

## Data Flow

Connection flow:

1. A caller outside `transport` selects the peer URL to use.
2. The caller asks `transport` to connect to that single URL with the selected
   root mode and relevant SFTP timeout and keep-alive settings.
3. The scheme dispatcher chooses the local or SSH/SFTP backend.
4. The backend verifies or creates the selected peer root according to
   `TransportRootMode`.
5. The backend establishes a connected handle rooted at the peer path.
6. The module returns a `TransportHandle` or a normalized `TransportError`.

Listing and stat flow:

1. A caller passes a root-relative `RelPath` to `list_dir` or `stat`.
2. The backend performs the native operation relative to the connected peer
   root.
3. The metadata adapter preserves reported entry names, converts supported file
   and directory metadata into `EntryMeta`, and omits unsupported entry types
   from directory listings.
4. `stat` treats missing paths, symlinks, and unsupported non-regular entries
   as `TransportError::not_found`.
5. Any backend failure is normalized before it leaves the module.

Read and write flow:

1. A caller opens a reader or writer for a root-relative `RelPath`.
2. The backend returns a stream over the native file or SFTP handle.
3. Opening a writer creates the target file and any missing parent directories
   required for that file.
4. The caller owns copy planning, safe replacement sequencing, retries,
   progress, and diagnostics around the stream.
5. Stream and close failures are returned as normalized transport errors.

Mutation flow:

1. A caller requests a primitive filesystem effect such as create, delete,
   rename-without-overwrite, or setting modification time.
2. The backend performs only that requested effect relative to the connected
   peer root.
3. If a native rename operation would overwrite by default, the backend first
   prevents overwrite and reports a normalized error when the destination
   exists.
4. The module reports success or a normalized transport error. It does not
   decide whether the effect should happen.

## Dependencies

`transport` consumes root contracts for `PeerUrl`, `RelPath`, `EntryMeta`,
`EntryKind`, `Timestamp`, `TransportError`, `TransportRootMode`, and run
timeout and keep-alive settings. It may depend on standard Rust I/O traits,
host filesystem APIs, and SSH/SFTP libraries needed to implement the supported
schemes.

No sibling implementation should be imported. In particular:

- `cli` owns command-line peer operand parsing;
- `peer` owns fallback URL selection, peer role application, and startup
  peer-session construction;
- `snapshot` owns SQLite rows and durable peer metadata;
- `sync` owns traversal and sync decisions;
- `operations` owns safe replacement, SWAP, BAK, TMP, and dry-run mutation
  sequencing;
- `runtime` owns scheduling, retries, diagnostics, and progress output.

## Child Modules

`transport` should remain a leaf module. The source context requires two
backend implementations with matching behavior, but the module surface is one
cohesive connected filesystem contract. Private implementation files such as
`local`, `sftp`, `metadata`, `streams`, `mutations`, and `errors` are enough to
keep future work narrow without adding generated child architecture files.
