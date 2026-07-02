# Local File Metadata

## Risk

The local filesystem transport must set the winning modification time on files
and directories. Rust standard library support varies by operation shape, so the
product needs a portable crate call for path-based mtime updates.

## Experiment

`plan/experiments/local-file-metadata` uses `filetime` `0.2.25`. It creates a
temporary file and directory, calls:

- `FileTime::from_unix_time(seconds, nanoseconds)`;
- `set_file_mtime(path, file_time)`;
- `FileTime::from_last_modification_time(&metadata)`.

The experiment asserts that both a file and a directory round-trip their
seconds and microsecond components on this machine.

## Proven Package

- `filetime` `0.2.25`

## Notes For Later Code

Use this for local path mtime updates after a copy has been renamed into place.
The SFTP experiment separately proves only second-level SFTP mtime through the
local fixture.

