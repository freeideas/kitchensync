# SftpTransportOperations:

## Purpose

SftpTransportOperations performs peer filesystem operations for an already
connected `sftp://` peer. It is the SFTP backend used by the parent transport
facade, and every operation runs through the established SSH/SFTP connection for
that peer.

The caller supplies a connected SFTP peer root handle and paths relative to
that root. The public interface must expose that connected root handle so each
root-bound operation receives both the established SSH/SFTP session for the
peer and the remote peer root path to resolve paths under. This child does not
choose peer URLs, open SSH sessions, authenticate users, verify host keys,
reconnect after failure, or decide sync outcomes.

## Responsibilities

SftpTransportOperations exposes the transport filesystem operations needed by
the parent facade for an SFTP peer. Every operation that resolves a
peer-relative path receives the connected SFTP peer root handle:

- `list_dir(root, path)` lists the immediate children of `path` through SFTP.
- `stat(root, path)` reads SFTP metadata for a regular file or directory.
- `open_read(root, path)`, `read(handle, max_bytes)`, and `close_read(handle)`
  stream bytes from an existing regular file through SFTP.
- `open_write(root, path)`, `write(handle, bytes)`, and `close_write(handle)`
  stream bytes to a file through SFTP, creating the file and any missing
  parent directories required by the parent operation surface.
- `rename(root, src, dst)` moves an entry within the same SFTP filesystem when
  `dst` does not already exist.
- `delete_file(root, path)` removes a regular file through SFTP.
- `create_dir(root, path)` creates a directory and any missing parent
  directories through SFTP.
- `delete_dir(root, path)` removes an empty directory through SFTP.
- `set_mod_time(root, path, time)` updates the modification time of a regular
  file or directory through SFTP.

Directory listings and metadata returned across this boundary use the parent's
transport shape: names are child names, regular files report modification time,
byte size, and non-directory type, and directories report modification time,
byte size `-1`, and directory type. Entries that SFTP reports as symbolic links
or as non-regular non-directory filesystem objects are omitted from
`list_dir(root, path)` and are treated as `not_found` by `stat(root, path)`.

All errors returned across this boundary use the parent's transport error
categories: `not_found`, `permission_denied`, or `io_error`. Network failures
while performing SFTP transport operations, including a broken SSH session,
socket failure, timeout reported by the SSH/SFTP library, or lost SFTP channel,
are reported as `io_error`.

Open read and write handles belong to the SFTP connection that created them.
Reads return only file content bytes and then EOF. Writes send the supplied
bytes in order, and `close_write(handle)` flushes and closes the remote file
handle before reporting success.

## Boundaries

SftpTransportOperations is not a connection owner. It does not parse or
normalize URLs, select fallback URLs, resolve credentials, read `known_hosts`,
verify host keys, authenticate passwords or saved keys, or create the initial
SFTP session. Those behaviors belong before this child receives the connected
SFTP peer root handle from its caller.

SftpTransportOperations does not implement local `file://` filesystem behavior
and does not decide parity between local and SFTP outcomes. Its job is to make
the SFTP side obey the operation names, metadata fields, handle behavior, and
error categories required by the parent facade.

SftpTransportOperations does not require remote rename to overwrite an existing
destination. Callers that replace existing data must use a sequence that works
when the SFTP server rejects overwrite.

The invariant for every operation is that paths stay within the connected SFTP
peer root and are interpreted as peer-relative transport paths. The child must
not turn symbolic links into traversable paths for the parent operation surface;
symbolic links are omitted or reported as absent as described above.
