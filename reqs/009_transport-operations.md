# 009_transport-operations: Local and SFTP filesystem operations

## Behavior
This concern derives from `specs/sync.md` section "Peer Transports",
`specs/database.md` opening section, `extart/ephemeral-sftp-server.py`, and
`plan/sftp-client.md` and `plan/local-file-metadata.md`. It covers the common
transport operation surface for `file://` and `sftp://` peers, local and SFTP
metadata behavior, same-filesystem rename to a missing destination, regular
file and directory listing semantics, symlink and special-file omission, and
transport-neutral error categories.

## $REQ_IDs

- `009.1` -- After connection, KitchenSync performs filesystem operations for a `file://` peer through the local filesystem.
- `009.2` -- After connection, KitchenSync performs filesystem operations for an `sftp://` peer through SSH/SFTP.
- `009.3` -- `list_dir(path)` lists only immediate children of `path`.
- `009.4` -- `list_dir(path)` reports each regular-file child with its name, modification time, byte size, and non-directory type.
- `009.5` -- `list_dir(path)` reports each directory child with its name, modification time, byte size `-1`, and directory type.
- `009.6` -- `list_dir(path)` omits symbolic links.
- `009.7` -- `list_dir(path)` omits devices, FIFOs, sockets, and other non-regular non-directory entries.
- `009.8` -- `stat(path)` reports modification time, byte size, and directory type for an existing regular file or directory.
- `009.9` -- `stat(path)` returns the not-found category for a missing path.
- `009.10` -- `stat(path)` returns the not-found category for a symbolic link.
- `009.11` -- `stat(path)` returns the not-found category for a device, FIFO, socket, or other non-regular non-directory entry.
- `009.12` -- `open_read(path)` opens a regular file for streaming reads.
- `009.13` -- `read(handle, max_bytes)` returns up to `max_bytes` of the next file-content bytes for an open read handle.
- `009.14` -- `read(handle, max_bytes)` returns EOF after all file content for an open read handle has been returned.
- `009.15` -- `close_read(handle)` closes an open read handle.
- `009.16` -- `open_write(path)` opens a file for streaming write.
- `009.17` -- `open_write(path)` creates the target file when it does not exist.
- `009.18` -- `open_write(path)` creates missing parent directories for the target file.
- `009.19` -- `write(handle, bytes)` writes the given bytes to an open write handle.
- `009.20` -- `close_write(handle)` flushes and closes an open write handle.
- `009.21` -- `rename(src, dst)` moves an entry within the same filesystem when `dst` does not already exist.
- `009.22` -- KitchenSync does not require `rename(src, dst)` to overwrite an existing `dst`.
- `009.23` -- `delete_file(path)` removes a file.
- `009.24` -- `create_dir(path)` creates a directory and any missing parent directories.
- `009.25` -- `delete_dir(path)` removes an empty directory.
- `009.26` -- `set_mod_time(path, time)` updates the modification time of a regular file.
- `009.27` -- `set_mod_time(path, time)` updates the modification time of a directory.
- `009.28` -- Transport operations report errors using the categories not found, permission denied, and I/O error.
- `009.29` -- Network failures during `sftp://` transport operations are reported as I/O errors.
- `009.30` -- The same transport error category produces the same sync outcome for `file://` and `sftp://` peers.

## Notes
This file covers primitive peer operations. Higher-level copy sequencing,
staging, retry, and sync decisions belong to later categories.
