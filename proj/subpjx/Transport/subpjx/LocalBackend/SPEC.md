# LocalBackend:

## Purpose

LocalBackend is the per-scheme adapter that implements Transport's uniform
filesystem operation set over a local `file://` peer. Its winning URL names a
directory on the machine running the program, and every operation it exposes maps
to ordinary local filesystem access against that directory. The parent Transport
owns URL selection and the uniform interface; once it has chosen a winning
`file://` URL for a peer, it delegates that peer's root handling and every
filesystem operation to this child. LocalBackend exists so that the `file://`
half of Transport's promise -- that a `file://` peer and an `sftp://` peer with
identical contents yield identical results -- is met by a single component that
behaves exactly like its SFTP sibling across the same boundary.

LocalBackend is the local counterpart of SftpBackend. The two implement the same
operations with the same shapes and the same three error categories; only the
underlying access differs (local filesystem here, an SFTP connection there).

## Responsibilities

Peer root handling for a `file://` URL:

- In a normal run, create a missing peer root directory, creating any missing
  parent directories along the way (005.9, 005.11).
- In a normal run, treat a URL whose root directory cannot be created as failed
  (005.12).
- In `--dry-run`, do not create a missing peer root directory; treat a URL whose
  root directory does not already exist as failed, that is, unreachable, for that
  run, and never create the missing root or its parents (005.13, 005.14, 024.11).

The uniform filesystem operations, each over the peer's `file://` root:

- `list_dir(path)` returns each immediate child's `name`, `is_dir`, `mod_time`,
  and `byte_size`. `byte_size` is the file size in bytes for a regular file and
  `-1` for a directory (022.2, 022.3, 022.4).
- `list_dir(path)` silently omits symbolic links, special files, and any other
  non-regular entry, so its result contains only regular files and directories
  (022.15).
- `stat(path)` returns `mod_time`, `byte_size`, and `is_dir` for an existing
  regular file or directory, and returns "not found" when the path does not
  exist (022.5, 022.6).
- `stat(path)` returns "not found" for a symbolic link or special file, matching
  the `list_dir` omission rule (022.16).
- Streaming read: `read(handle, max_bytes)` returns the next chunk of bytes from
  an open file, or EOF at the end of the file (022.7).
- `open_write(path)` creates the target file and any missing parent directories
  (022.8).
- `create_dir(path)` creates the directory and any missing parent directories
  (022.9).
- `rename(src, dst)` moves `src` to `dst` when `dst` does not exist (022.10), and
  fails when `dst` already exists rather than overwriting it (022.11).
- `delete_file(path)` removes a file (022.12); `delete_dir(path)` removes an empty
  directory (022.13).
- `set_mod_time(path, time)` sets the modification time of a file or directory
  (022.14).

## Boundaries

Error obligations:

- Every operation reports failure using only the three categories not found,
  permission denied, and I/O error. LocalBackend maps each native filesystem
  error into exactly one of these categories and never surfaces a
  scheme-specific or platform-specific error to its caller (022.17).
- A failure that is not a missing path and not a permission rejection -- including
  any low-level I/O fault such as a device error -- surfaces as an I/O error
  (022.18). Although LocalBackend touches only the local filesystem, it reports in
  the same I/O-error category its SFTP sibling uses for a connection drop or
  timeout, so callers never match on scheme.

Invariants:

- The operation set, return shapes, the `byte_size` rule (`-1` for directories),
  the non-regular-entry omission rule, and the three error categories are
  identical to SftpBackend's, so a `file://` peer and an `sftp://` peer with
  identical contents produce identical observations across this boundary.
- `rename` never relies on rename-over-existing: a destination that already exists
  is always a failure, leaving any staged replacement to the callers that need it
  (022.11).
- In `--dry-run`, LocalBackend makes no change to the peer filesystem: it creates
  no root, no parent, and no file, and an absent root is reported as unreachable
  rather than created (005.13, 005.14, 024.11).

What LocalBackend does not do:

- It does not select a peer's winning URL among primary and fallback URLs, decide
  per-peer reachability across all of a peer's URLs, or hold the per-run singleton
  connection state; those stay with the parent Transport. LocalBackend acts on the
  one `file://` URL Transport hands it.
- It does not normalize URLs; canonical identity is `UrlNormalize`'s concern.
  LocalBackend receives an already-canonical `file://` URL.
- It does not parse the command line, build the bounded-buffer copy loop, do SWAP
  staging, count reachable peers, decide canon, or emit progress output. It
  provides only the categorized chunk-level and metadata operations its callers
  build on, and returns categorized errors and the per-URL root verdict.
- It holds no sibling dependency; it is a leaf backend used by the parent.
