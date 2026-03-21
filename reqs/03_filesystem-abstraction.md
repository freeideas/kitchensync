# Filesystem Abstraction

Peer filesystem trait that all sync logic operates through.

## $REQ_FSA_001: Single Trait for All Filesystem Operations
**Source:** ./specs/sync.md (Section: "Peer Filesystem Abstraction")

All sync logic operates through a single trait (interface) that both `file://` and `sftp://` implement. No protocol-specific code exists outside the trait implementations.

## $REQ_FSA_002: List Directory Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`list_dir(path)` lists immediate children returning name, is_dir, mod_time, and byte_size (file size in bytes for files, −1 for directories).

## $REQ_FSA_003: Stat Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`stat(path)` returns mod_time, byte_size, and is_dir; or "not found".

## $REQ_FSA_004: Read File Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`read_file(path)` opens a file for streaming read.

## $REQ_FSA_005: Write File Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`write_file(path, stream)` creates or overwrites a file from a stream, creating parent directories as needed.

## $REQ_FSA_006: Rename Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`rename(src, dst)` performs a same-filesystem rename (for XFER → final swap).

## $REQ_FSA_007: Delete File Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`delete_file(path)` removes a file.

## $REQ_FSA_008: Create Directory Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`create_dir(path)` creates a directory and parents as needed.

## $REQ_FSA_009: Delete Directory Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`delete_dir(path)` removes an empty directory.

## $REQ_FSA_010: Set Mod Time Operation
**Source:** ./specs/sync.md (Section: "Required Operations")

`set_mod_time(path, time)` sets a file or directory's modification time.

## $REQ_FSA_011: Uniform Error Types
**Source:** ./specs/sync.md (Section: "Error Semantics")

All operations return the same error types regardless of transport: not found, permission denied, I/O error. The sync logic never matches on transport-specific errors.

## $REQ_FSA_012: Network Failures as IO Errors
**Source:** ./specs/sync.md (Section: "Error Semantics")

Network failures (connection drop, timeout) surface as I/O errors. The sync logic does not distinguish disk failures from network failures.
