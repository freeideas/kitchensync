# Local Filesystem Operations

## Risk

The local peer transport must expose filesystem operations with the same shape
as the SFTP transport: create missing parents, list immediate regular entries,
stream file reads and writes, rename only to a missing destination, displace a
directory as one rename, and delete files or empty directories. Rust standard
library calls are available, but `rename` behavior around an existing
destination is host-dependent enough that product code must not expose a raw
overwrite.

## Experiment

`plan/experiments/local-fs-ops` uses only Rust standard library calls. It
asserts:

- `std::fs::create_dir_all` creates missing parents;
- `std::fs::read_dir`, `DirEntry::file_type`, and `DirEntry::metadata` list an
  immediate directory entry and allow directory byte size to be represented as
  `-1`;
- `std::fs::File::create`, `Write::write_all`, `Write::flush`,
  `std::fs::File::open`, and `Read::read_to_string` provide streaming-shaped
  local file I/O;
- a local helper using `Path::try_exists` before `std::fs::rename` rejects an
  existing destination with `ErrorKind::AlreadyExists` and preserves both files;
- `std::fs::rename` moves a directory to a missing BAK path and preserves its
  subtree;
- `std::fs::remove_file`, `std::fs::remove_dir`, and `std::fs::remove_dir_all`
  remove the test files and directories.

## Proven Packages

None. This experiment uses the Rust standard library.

## Notes For Later Code

Use an explicit destination-exists check before local `std::fs::rename` to keep
the transport rule that `rename(src, dst)` does not overwrite `dst`. The
experiment proves local regular file and directory operations; modification
time updates are covered separately in `local-file-metadata.md`.
