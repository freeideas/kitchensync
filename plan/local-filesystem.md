# Local Filesystem

## Risk

The local `file://` transport depends on host filesystem behavior for
modification times and same-filesystem renames. These are outside Rust package
APIs.

## Experiment

`experiments/local-filesystem` is a Rust mini-project with no downloaded
packages. It uses only `std`.

The experiment creates files and directories under the OS temporary directory,
sets a file modification time, renames a directory tree, and renames one file
over an existing file.

## Proved Calls

- `File::set_modified(SystemTime)` set a file modification time with
  microsecond precision on this machine.
- `std::fs::metadata(path)?.modified()` read back the same microsecond value.
- `std::fs::rename(dir, moved_dir)` moved a directory as one tree and preserved
  its child file.
- On this Linux machine, `std::fs::rename(src_file, existing_file)` overwrote
  the existing destination and removed the source path.

## Notes

The overwrite result is an observed local OS behavior, not a portable rule for
all target platforms. The product should follow the spec's SWAP flow instead of
depending on overwrite.
