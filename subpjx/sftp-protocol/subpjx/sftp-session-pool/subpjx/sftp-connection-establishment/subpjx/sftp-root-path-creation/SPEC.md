# SFTP Root Path Creation

## Purpose
Ensure an SFTP directory path exists by checking the path and creating missing parent directories through SFTP.

## Public API
Data shapes:

- `SftpFilesystem`: an active SFTP filesystem connection
- `RootPath`: SFTP path to a directory
- `RootPathStatus`: `exists` or `created`

Operations:

- `ensure_root_path(sftp_filesystem, root_path) -> RootPathStatus`: verify that `root_path` exists as a directory, creating it and any missing parents when necessary.

## Behavior
`ensure_root_path` checks whether `root_path` exists through SFTP.

If `root_path` already exists as a directory, it returns `exists`.

If `root_path` does not exist, `ensure_root_path` creates each missing parent directory and then `root_path` through SFTP before returning `created`.

Creation is idempotent for directories that already exist.

## Errors
Root path creation fails if an existing path component is not a directory.

Root path creation fails if any required directory cannot be created.

SFTP protocol failure or SFTP I/O failure is reported as `io_error`.

## Anchoring
`SftpFilesystem`, SFTP path lookup, directory status, directory creation, and SFTP protocol failure are anchored in `draft-ietf-secsh-filexfer`.

`RootPath` is anchored in SFTP filesystem path semantics from `draft-ietf-secsh-filexfer`.

`io_error` is anchored in SSH/SFTP I/O failure semantics.
