# 009_transport-operations: Local and SFTP filesystem operations

## Behavior
This concern derives from `specs/sync.md` section "Peer Transports",
`specs/database.md` opening section, `extart/ephemeral-sftp-server.py`,
`plan/sftp-client.md`, `plan/local-fs-ops.md`, and
`plan/local-file-metadata.md`. It covers the common transport operation surface
for `file://` and `sftp://` peers, local and SFTP metadata behavior,
same-filesystem rename to a missing destination, regular file and directory
listing semantics, symlink and special-file omission, and transport-neutral
error categories.

## $REQ_IDs

- `009.1` -- After connection, KitchenSync performs filesystem operations for a `file://` peer through the local filesystem.
- `009.2` -- After connection, KitchenSync performs filesystem operations for an `sftp://` peer through SSH/SFTP.
- `009.3` -- After connection, KitchenSync scopes root-bound transport operations to the connected peer root.
- `009.4` -- After connection, KitchenSync supplies root-bound transport operations with paths relative to the connected peer root.
- `009.5` -- `list_dir(path)` lists only immediate children of `path`.
- `009.6` -- `list_dir(path)` reports each regular-file child with its name.
- `009.7` -- `list_dir(path)` reports each regular-file child with non-directory type.
- `009.8` -- `list_dir(path)` reports each regular-file child with its modification time.
- `009.9` -- `list_dir(path)` reports each regular-file child with its byte size.
- `009.10` -- `list_dir(path)` reports each directory child with its name.
- `009.11` -- `list_dir(path)` reports each directory child with directory type.
- `009.12` -- `list_dir(path)` reports each directory child with its modification time.
- `009.13` -- `list_dir(path)` reports each directory child with byte size `-1`.
- `009.14` -- `list_dir(path)` omits symbolic links.
- `009.15` -- `list_dir(path)` omits devices, FIFOs, sockets, and other non-regular non-directory entries.
- `009.16` -- `stat(path)` reports non-directory type for an existing regular file.
- `009.17` -- `stat(path)` reports modification time for an existing regular file.
- `009.18` -- `stat(path)` reports byte size for an existing regular file.
- `009.19` -- `stat(path)` reports directory type for an existing directory.
- `009.20` -- `stat(path)` reports modification time for an existing directory.
- `009.21` -- `stat(path)` reports byte size for an existing directory.
- `009.22` -- `stat(path)` returns the not-found category for a missing path.
- `009.23` -- `stat(path)` returns the not-found category for a symbolic link.
- `009.24` -- `stat(path)` returns the not-found category for a device, FIFO, socket, or other non-regular non-directory entry.
- `009.25` -- `open_read(path)` opens a regular file for streaming reads.
- `009.26` -- `read(handle, max_bytes)` returns up to `max_bytes` of the next file-content bytes for an open read handle.
- `009.27` -- `read(handle, max_bytes)` returns EOF after all file content for an open read handle has been returned.
- `009.28` -- `close_read(handle)` closes an open read handle.
- `009.29` -- `open_write(path)` opens a file for streaming write.
- `009.30` -- `open_write(path)` creates the target file when it does not exist.
- `009.31` -- `open_write(path)` creates missing parent directories for the target file.
- `009.32` -- `write(handle, bytes)` writes the given bytes to an open write handle.
- `009.33` -- `close_write(handle)` flushes an open write handle.
- `009.34` -- `close_write(handle)` closes an open write handle.
- `009.35` -- `rename(src, dst)` moves an entry within the same filesystem when `dst` does not already exist.
- `009.36` -- `rename(src, dst)` preserves a directory subtree when moving that directory to a missing destination.
- `009.37` -- KitchenSync does not rely on `rename(src, dst)` overwriting an existing `dst`.
- `009.38` -- `delete_file(path)` removes a file.
- `009.39` -- `create_dir(path)` creates a directory.
- `009.40` -- `create_dir(path)` creates any missing parent directories.
- `009.41` -- `delete_dir(path)` removes an empty directory.
- `009.42` -- `set_mod_time(path, time)` updates the modification time of a regular file.
- `009.43` -- `set_mod_time(path, time)` updates the modification time of a directory.
- `009.44` -- Transport operations report not-found errors using the not-found category.
- `009.45` -- Transport operations report permission-denied errors using the permission-denied category.
- `009.46` -- Transport operations report other I/O errors using the I/O error category.
- `009.47` -- Network failures during `sftp://` transport operations are reported as I/O errors.
- `009.48` -- The same transport error category produces the same sync outcome for `file://` and `sftp://` peers.

## Notes
This file covers primitive peer operations. Higher-level copy sequencing,
staging, retry, snapshot replacement, and sync decisions belong to other
categories.
