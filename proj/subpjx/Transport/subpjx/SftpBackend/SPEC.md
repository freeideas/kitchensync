# SftpBackend:

## Purpose

SftpBackend is the per-scheme adapter that the Transport facade delegates to for
every `sftp://` peer. It owns two things for that scheme: establishing an
authenticated, host-key-verified SSH connection to one peer URL within a bounded
handshake, and then carrying out the uniform filesystem operation set over that
live connection. The Transport facade chooses which URL to try and in what
order; SftpBackend is handed one already-normalized `sftp://` URL at a time and
either returns a usable connection or reports that this URL failed. Once the
facade has a winning connection, every later filesystem call for that peer runs
through this backend.

SftpBackend exists so that the operation set, the error categories, and the
connection rules are identical for an `sftp://` peer and a `file://` peer. Its
sibling LocalBackend implements the same operation set over the local scheme;
the two must return the same shapes and the same error categories so that a
`file://` peer and an `sftp://` peer with identical contents produce identical
results. SftpBackend never decides peer reachability across multiple URLs,
never counts reachable peers, and never emits output; it answers one URL's
connect attempt and one operation at a time and returns the result to the
facade.

## Responsibilities

Connect one `sftp://` URL:

- Authenticate by trying credential sources in this exact fixed order, skipping
  any source that is absent and falling through to the next source when the host
  rejects one: first the inline URL password, then the SSH agent named by the
  `SSH_AUTH_SOCK` environment variable, then `~/.ssh/id_ed25519`, then
  `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa` (004.1 through 004.7).
- Percent-decode an inline URL password before using it for authentication, so
  `%40` becomes `@` and `%3A` becomes `:` (004.10).
- Verify the host key against `~/.ssh/known_hosts`: a host whose key matches its
  entry passes, and a host absent from that file is rejected (004.8, 004.9).
- Bound the SSH handshake by the connection timeout: `--timeout-conn` is the
  default bound, and a URL's own `timeout-conn` query parameter overrides it for
  that URL's handshake. When the handshake does not complete within its bound,
  abandon this URL and report it as failed so the facade can try the next URL
  (005.6, 005.7, 005.8).
- Handle the peer root for the URL. In a normal run, create a missing root
  directory and any missing parent directories; treat a URL whose root cannot be
  created as failed. In `--dry-run`, do not create a missing root, and treat a
  URL whose root does not already exist as failed for that run (005.10, 005.11,
  005.12, 005.13, 005.14, 024.11).

Uniform filesystem operations over the established connection:

- `list_dir(path)` returns each immediate child's `name`, `is_dir`, `mod_time`,
  and `byte_size`, where `byte_size` is the file size in bytes for a regular file
  and `-1` for a directory. It silently omits symbolic links, special files, and
  any other non-regular entry (022.2, 022.3, 022.4, 022.15).
- `stat(path)` returns `mod_time`, `byte_size`, and `is_dir` for an existing
  regular file or directory, and "not found" when the path does not exist or
  names a symbolic link or special file (022.5, 022.6, 022.16).
- `read(handle, max_bytes)` returns the next chunk of bytes, or EOF at the end of
  the file (022.7).
- `open_write(path)` creates the target file and any missing parent directories
  (022.8).
- `create_dir(path)` creates the directory and any missing parent directories
  (022.9).
- `rename(src, dst)` moves `src` to `dst` when `dst` does not exist, and fails
  when `dst` already exists (022.10, 022.11).
- `delete_file(path)` removes a file; `delete_dir(path)` removes an empty
  directory (022.12, 022.13).
- `set_mod_time(path, time)` sets the modification time of a file or directory
  (022.14).

## Boundaries

Error obligations:

- Every operation and every connect attempt reports failure using only the
  categories not found, permission denied, and I/O error (022.17).
- A network failure such as a connection drop or a timeout surfaces as an I/O
  error, never as an SFTP- or SSH-specific error, so callers never match on
  scheme (022.18).

Invariants:

- The credential sources are always tried in the fixed order above; an absent
  source is skipped without consuming the attempt, and a rejected source falls
  through to the next.
- An unknown host -- one with no matching `~/.ssh/known_hosts` entry -- is always
  rejected; SftpBackend never accepts a host on first sight.
- The handshake is always bounded by the effective connection timeout, and a URL
  that exceeds its bound is reported as failed rather than waited on.
- The operation set, return shapes, omission rule, and error categories match
  LocalBackend exactly, so identical contents on the two schemes yield identical
  results.
- `rename` never relies on rename-over-existing; a destination that already
  exists is a failure.

What SftpBackend does not do:

- It does not choose among a peer's primary and fallback URLs or decide peer
  reachability across them; it answers a single URL's connect attempt and the
  facade selects the winning URL.
- It does not normalize URLs; it consumes an already-normalized `sftp://` URL.
- It does not build the streaming copy loop, retry copies, or do SWAP staging; it
  provides only the chunk-level read and write primitives.
- It does not own the global dry-run decision; it connects exactly as a normal
  run does and honors dry-run only by not creating a missing root and treating an
  absent root as a failed URL for that run.
- It does not emit progress or diagnostic output; outcomes are returned to the
  facade.
