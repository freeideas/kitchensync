# Staged File Transfer

A Java 21 library for staged file replacement and displacement on filesystem-like
backends. It streams file content through a bounded concurrent pipeline, writes
to a temporary path near the destination, atomically renames the staged file into
place, moves displaced files and directories into nearby backup directories, and
removes expired staging and backup directories.

The library is for file operations only. It does not parse command lines or
URLs, open network connections, authenticate, verify host keys, manage
connection pools, choose sync winners, traverse a whole sync tree, apply ignore
rules, store snapshots, generate timestamps, or log diagnostics. Callers provide
already-open filesystem handles, relative paths, timestamps, UUIDs, and file
metadata, then decide what to do with the returned result.

## Public API

The API may use normal Java classes, records, interfaces, or equivalent types,
but it must expose this behavior.

### Data Shapes

`TransferFilesystem`

An interface implemented by the caller's filesystem backend. Paths passed to it
are slash-separated paths relative to that backend's root.

| Operation | Behavior |
| --- | --- |
| `list_dir(path) -> List<Entry>` | Lists immediate children only. |
| `stat(path) -> Entry` | Returns one regular file or directory, or `not_found`. |
| `open_read(path) -> ReadHandle` | Opens a regular file for streaming read. |
| `read(handle, max_bytes) -> bytes or EOF` | Reads the next chunk. EOF is distinct from an empty byte array. |
| `close_read(handle)` | Closes a read handle. It is safe to call after a failed read. |
| `open_write(path) -> WriteHandle` | Opens a regular file for streaming write, creating parent directories as needed. |
| `write(handle, bytes)` | Writes bytes to the open handle. |
| `close_write(handle)` | Flushes and closes the write handle. |
| `rename(src, dst)` | Same-filesystem rename. |
| `delete_file(path)` | Removes a regular file. |
| `create_dir(path)` | Creates a directory and any missing parents. Succeeds if the directory already exists. |
| `delete_dir(path)` | Removes an empty directory. |
| `set_mod_time(path, time)` | Sets the modification time of a file or directory. |

`rename(src, dst)` must not be assumed to overwrite an existing destination.
The library must arrange copy and displacement operations so they work on
backends, including SFTP backends, where rename-to-existing fails.

The library never closes or disposes a `TransferFilesystem`. Borrowing pooled
connections, returning pooled connections, and closing backend sessions are
caller responsibilities.

`Entry`

| Field | Meaning |
| --- | --- |
| `name` | Final path component only. |
| `kind` | `file` or `directory`. |
| `mod_time` | Modification time as a UTC instant. |
| `byte_size` | File size in bytes, or `-1` for directories. |

`StagingTimestamp`

A timestamp string in this exact filesystem-safe UTC format:

```text
YYYY-MM-DD_HH-mm-ss_ffffffZ
```

The library validates this format but does not generate timestamp values.

`TransferId`

A UUID string used as the unique transfer directory below a timestamped TMP
directory. Invalid UUID text is rejected.

`CopyRequest`

| Field | Meaning |
| --- | --- |
| `source` | Filesystem handle to read from. |
| `source_path` | Relative path of the source file. |
| `destination` | Filesystem handle to write to. |
| `destination_path` | Relative final path on the destination filesystem. |
| `winning_mod_time` | Modification time to set on the destination file after the final rename. |
| `staging_timestamp` | Timestamp directory name used for TMP and BAK paths created by this copy. |
| `transfer_id` | UUID directory name used below the TMP timestamp directory. |
| `chunk_size` | Maximum bytes per read. Positive integer. |
| `channel_capacity` | Maximum chunks buffered between reader and writer. Positive integer. |

`DisplaceRequest`

| Field | Meaning |
| --- | --- |
| `filesystem` | Filesystem handle containing the entry to displace. |
| `path` | Relative path of the file or directory to move aside. |
| `staging_timestamp` | Timestamp directory name used for the BAK destination. |

`CleanupRequest`

| Field | Meaning |
| --- | --- |
| `filesystem` | Filesystem handle to clean. |
| `directory_path` | Relative directory whose `.kitchensync` metadata directory should be inspected. Empty string means the filesystem root. |
| `bak_cutoff_exclusive` | Delete BAK timestamp directories older than this timestamp. |
| `tmp_cutoff_exclusive` | Delete TMP timestamp directories older than this timestamp. |

`OperationResult`

| Field | Meaning |
| --- | --- |
| `status` | `success`, `failed`, or `partial_success`. |
| `created_paths` | Metadata paths created by the operation, in operation order. |
| `removed_paths` | Paths removed by cleanup, in operation order. |
| `backup_path` | BAK path used for a displaced destination, when applicable. |
| `temporary_path` | TMP file path used by a copy, when applicable. |
| `final_path` | Final destination path for a successful or partially successful copy. |
| `error` | Error category when `status` is not `success`. |

### Paths

Public operations use normalized relative paths:

- Empty string is valid only when it names the filesystem root for
  `CleanupRequest.directory_path`.
- File and directory paths must not start with `/`.
- File and directory paths must not end with `/`.
- Paths must not contain empty segments, `.` segments, `..` segments,
  backslash, or NUL.
- The final segment of a copy or displacement path is the basename used inside
  TMP or BAK.

Metadata paths are constructed beside the affected entry:

| Purpose | Path shape |
| --- | --- |
| Copy staging | `<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>` |
| Displacement backup | `<entry-parent>/.kitchensync/BAK/<timestamp>/<basename>` |

For root-level entries, `<target-parent>` or `<entry-parent>` is omitted, so a
root file named `a.txt` stages at
`.kitchensync/TMP/<timestamp>/<uuid>/a.txt`.

### Operations

`StagedFileTransfer.copy_file(request) -> OperationResult`

Copies one regular file from `request.source_path` to
`request.destination_path`.

Required behavior:

1. Create the destination TMP parent directory.
2. Stream source content into the TMP file using two concurrent tasks connected
   by a bounded channel: one task reads chunks from the source and pushes them
   into the channel; the other task pulls chunks and writes them to the
   destination TMP file. A single read-then-write loop is not allowed.
3. Close both read and write handles. Handles are closed even after read, write,
   or close failures when the backend allows it.
4. If a file or directory already exists at `destination_path`, displace it to
   BAK using the same `staging_timestamp`. This is a rename to a new BAK path,
   not a delete, and must happen before the final rename.
5. Rename the TMP file to `destination_path`.
6. Set `destination_path` modification time to `winning_mod_time`.
7. Remove empty TMP UUID and timestamp directories created for this copy when
   possible.

If transfer fails before the final rename, the library deletes the TMP file and
its empty TMP directories when possible. The original destination entry, if any,
must not be displaced before the TMP write has completed successfully.

If displacement fails, the final rename is not attempted. The TMP file is
deleted when possible. The original destination entry remains in place.

If the final rename succeeds but `set_mod_time` fails, the result is
`partial_success` with error `set_mod_time_failed`; the copied file remains in
place and is not undone.

`StagedFileTransfer.displace(request) -> OperationResult`

Moves a file or directory at `request.path` to its BAK path. The operation
creates the BAK timestamp directory and any missing parents before renaming. A
directory is moved as one rename, preserving its whole subtree. If the source
path is not found, the operation returns `success` with no backup path.

`StagedFileTransfer.cleanup_expired(request) -> OperationResult`

Looks for `.kitchensync/BAK/` and `.kitchensync/TMP/` below
`request.directory_path`, then recursively deletes timestamp directories older
than the corresponding cutoff. Directory names that do not parse as
`StagingTimestamp` are ignored. Missing `.kitchensync`, `BAK`, or `TMP`
directories are treated as already clean.

Recursive deletion deletes files before their containing directories. If one
expired timestamp directory cannot be fully removed, cleanup continues with the
remaining expired timestamp directories and returns `partial_success`.

## Observable Behavior

- Public operations are deterministic for the same filesystem behavior,
  requests, timestamps, and UUIDs.
- Public operations do not write to stdout or stderr.
- Source and destination filesystems may be the same object.
- A copy where source and destination are the same path on the same filesystem
  is invalid input.
- The copy operation does not buffer the full file in memory.
- The bounded channel applies backpressure: the reader blocks when the channel
  is full and the writer blocks when it is empty.
- Existing destination content is not moved to BAK until the full TMP file has
  been written and closed successfully.
- BAK and TMP path construction preserves the final basename exactly.
- Cleanup uses the timestamp directory name, not filesystem modification times,
  to determine age.

## Error Behavior

Invalid requests fail before filesystem mutation with one of these categories:

| Category | Meaning |
| --- | --- |
| `invalid_path` | A public path violates the path rules above. |
| `invalid_timestamp` | A timestamp does not match `YYYY-MM-DD_HH-mm-ss_ffffffZ` or is not a valid UTC time. |
| `invalid_transfer_id` | A transfer ID is not a valid UUID string. |
| `invalid_settings` | `chunk_size` or `channel_capacity` is not positive. |
| `same_source_and_destination` | Source and destination identify the same path on the same filesystem. |

Filesystem failures are reported with these categories:

| Category | Meaning |
| --- | --- |
| `not_found` | The source file or displacement source does not exist. |
| `permission_denied` | The backend denied a requested operation. |
| `io_error` | Read, write, rename, delete, stat, listing, or backend I/O failed. |
| `displacement_failed` | Existing destination content could not be moved to BAK during a copy. |
| `rename_failed` | TMP-to-final rename failed after the TMP file was written. |
| `set_mod_time_failed` | Final file is in place, but its modification time could not be set. |
| `cleanup_incomplete` | Cleanup could not remove every expired path it attempted. |

The library returns backend errors through these categories only. It does not
return authentication, host-key, URL parsing, snapshot, decision, or ignore-rule
errors because it performs none of those tasks.

## Examples

### Copy Into An Empty Destination

Input:

```text
source_path = "album/a.jpg"
destination_path = "album/a.jpg"
source content = bytes [01 02 03]
winning_mod_time = 2026-05-15T10:30:00Z
staging_timestamp = 2026-05-15_10-31-00_000001Z
transfer_id = 123e4567-e89b-12d3-a456-426614174000
```

Observable destination operations:

```text
create_dir("album/.kitchensync/TMP/2026-05-15_10-31-00_000001Z/123e4567-e89b-12d3-a456-426614174000")
write staged bytes to "album/.kitchensync/TMP/2026-05-15_10-31-00_000001Z/123e4567-e89b-12d3-a456-426614174000/a.jpg"
rename(
  "album/.kitchensync/TMP/2026-05-15_10-31-00_000001Z/123e4567-e89b-12d3-a456-426614174000/a.jpg",
  "album/a.jpg"
)
set_mod_time("album/a.jpg", 2026-05-15T10:30:00Z)
```

Result:

```text
status = success
final_path = "album/a.jpg"
temporary_path = "album/.kitchensync/TMP/2026-05-15_10-31-00_000001Z/123e4567-e89b-12d3-a456-426614174000/a.jpg"
backup_path = absent
```

### Copy Over An Existing File

Input:

```text
destination_path = "notes/todo.txt"
existing destination is a file
staging_timestamp = 2026-05-15_11-00-00_000002Z
transfer_id = 123e4567-e89b-12d3-a456-426614174111
```

Before the final rename, the existing destination is moved to:

```text
notes/.kitchensync/BAK/2026-05-15_11-00-00_000002Z/todo.txt
```

Result:

```text
status = success
final_path = "notes/todo.txt"
backup_path = "notes/.kitchensync/BAK/2026-05-15_11-00-00_000002Z/todo.txt"
```

### Displace A Directory

Input:

```text
path = "album/raw"
staging_timestamp = 2026-05-15_12-00-00_000003Z
```

Observable operation:

```text
rename("album/raw", "album/.kitchensync/BAK/2026-05-15_12-00-00_000003Z/raw")
```

The directory subtree is moved by that single rename.

### Cleanup

Input:

```text
directory_path = "album"
bak_cutoff_exclusive = 2026-05-01_00-00-00_000000Z
tmp_cutoff_exclusive = 2026-05-14_00-00-00_000000Z

existing metadata directories:
album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/
album/.kitchensync/BAK/2026-05-01_00-00-00_000000Z/
album/.kitchensync/TMP/2026-05-13_23-59-59_000000Z/
```

Output:

```text
removed_paths includes:
album/.kitchensync/BAK/2026-04-30_23-59-59_000000Z/
album/.kitchensync/TMP/2026-05-13_23-59-59_000000Z/

album/.kitchensync/BAK/2026-05-01_00-00-00_000000Z/ is retained
```

## Testing Requirements

Tests are black-box tests of the public API. They may use temporary local
directories or instrumented in-memory `TransferFilesystem` implementations. No
external service account, SFTP server, SSH key, known-hosts file, SQLite
database, or network access is required. The SFTP service account used by
transport tests is not used for this component.

Required scenarios:

- Copying binary content preserves bytes without text conversion.
- Copying uses a reader task and writer task connected by a bounded channel;
  tests must be able to observe that reading and writing overlap rather than
  alternating in one sequential loop.
- `chunk_size` and `channel_capacity` affect chunking and backpressure without
  changing copied content.
- TMP path construction matches the specified path shape for root-level and
  nested destination paths.
- Existing destination files and directories are moved to the specified BAK path
  before the final rename.
- A displacement of a directory is one rename of the directory path, not a
  recursive file-by-file move.
- The final copied file receives the supplied `winning_mod_time`.
- A failed read, write, or close before the final rename leaves the original
  destination entry in place and removes TMP files when possible.
- A displacement failure prevents the final rename and removes the TMP file when
  possible.
- A `set_mod_time` failure after the final rename returns `partial_success` and
  leaves the copied file in place.
- Cleanup deletes only expired timestamp directories under BAK and TMP, ignores
  non-timestamp directory names, and continues after one deletion failure.
- Invalid paths, invalid timestamps, invalid UUIDs, non-positive settings, and
  same-filesystem same-path copies report the specified errors.
- No public operation emits stdout or stderr.

Scenarios to avoid:

- Do not test command-line parsing, help text, peer role validation, URL
  normalization, fallback URL selection, or startup reachability.
- Do not test SSH authentication, host-key verification, SFTP sessions, SFTP
  connection pooling, or the required SFTP service account.
- Do not test sync conflict decisions, timestamp generation, snapshot schema,
  path hashing, tombstones, or descendant snapshot cascades.
- Do not test ignore-pattern parsing or ignore-file resolution.
- Do not test whole-tree traversal, listing-error subtree exclusion, or
  operation scheduling across multiple sync decisions.

## Semantic Anchors

This specification is anchored in the semantic source sections for file copy,
displacement to BAK, TMP staging, BAK directory retention, BAK/TMP cleanup
during traversal, timestamp text format, transport filesystem operations,
transfer error behavior, and the required bounded reader/writer transfer
pipeline.
