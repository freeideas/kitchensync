# Transport:

## Purpose

Transport is the single uniform filesystem layer that every other component uses
to touch a peer. It hides whether a peer is a local `file://` directory or a
remote `sftp://` server behind one common interface, so the rest of the program
never branches on scheme and never sees a scheme-specific error. A `file://`
peer and an `sftp://` peer with identical contents must yield identical sync
results (022.1).

Transport also owns everything that happens before a peer can be used: turning a
peer URL into its canonical identity (URL normalization), authenticating an SFTP
connection and verifying its host key, and selecting one winning URL from a
peer's primary plus fallback URLs. Once a peer has a winning URL, every later
operation for that peer goes through it. Concentrating all of this here keeps the
operation set, the error categories, and the connection rules consistent across
the whole run.

## Responsibilities

The operations Transport exposes across its boundary fall into three groups.

URL normalization and identity:

- Turn a peer URL into its canonical identity used for comparison and snapshot
  lookup by lowercasing the scheme and hostname, removing the default SFTP port
  22, collapsing consecutive slashes, removing a trailing slash, converting a
  bare path to a `file://` URL resolved to an absolute path from the current
  working directory, percent-decoding unreserved characters, stripping
  query-string parameters, and inserting the current OS user as the username for
  an SFTP URL that omits one (003.1 through 003.10).
- Honor the worked examples exactly: `c:/photos/` becomes `file:///c:/photos`,
  `./data` from `/home/user` becomes `file:///home/user/data`,
  `SFTP://Host:22/path/` becomes `sftp://host/path`, `sftp://host//docs/` becomes
  `sftp://host/docs`, `sftp://host/path?timeout-conn=60` becomes
  `sftp://host/path`, and `sftp://host/path` run as OS user `ace` becomes
  `sftp://ace@host/path` (003.11 through 003.16).

Authentication and connection establishment:

- Authenticate an SFTP connection by trying credential sources in this exact
  order, skipping any that is absent and falling through on rejection: the inline
  URL password, then the SSH agent named by `SSH_AUTH_SOCK`, then
  `~/.ssh/id_ed25519`, then `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa` (004.1
  through 004.7).
- Verify the SFTP host key against `~/.ssh/known_hosts`, accepting a matching
  host and rejecting a host that is absent from that file (004.8, 004.9).
- Percent-decode special characters in an inline SFTP password before
  authenticating, so `%40` becomes `@` and `%3A` becomes `:` (004.10).
- Select a peer's winning URL by trying the primary URL first, then each fallback
  URL in listed order, taking the first URL that connects and not trying the
  rest, and binding the peer to that winning URL for the remainder of the run
  (005.1 through 005.5).
- Bound an SFTP handshake with `--timeout-conn`, overridden by the URL's own
  `timeout-conn` parameter; on timeout, abandon that URL and try the next (005.6,
  005.7, 005.8).
- Connect to peers in `--dry-run` exactly as in a normal run: same URL ordering,
  SFTP authentication, host-key verification, and handshake timeouts, so
  reachability is decided the same way; only peer-side root creation differs
  (024.1).
- Handle the peer root per URL: in a normal run create a missing root and any
  missing parents for both `file://` and `sftp://`, and treat a URL whose root
  cannot be created as failed; in `--dry-run` do not create a missing root and
  treat a URL whose root does not already exist as unreachable for that run
  (005.9 through 005.14, 024.11).
- Report a peer whose every URL fails as unreachable for the run (005.15).

Uniform per-peer filesystem operations (over the winning URL, for both schemes):

- `list_dir(path)` returns each immediate child's name, `is_dir`, `mod_time`, and
  `byte_size`, where `byte_size` is the file size in bytes for a regular file and
  `-1` for a directory (022.2, 022.3, 022.4).
- `stat(path)` returns `mod_time`, `byte_size`, and `is_dir` for an existing
  regular file or directory, and "not found" when the path does not exist (022.5,
  022.6).
- Streaming read: `open_read`, `read(handle, max_bytes)` returning the next chunk
  of bytes or EOF, and `close_read` (022.7).
- Streaming write: `open_write(path)` creating the target file and any missing
  parent directories, `write`, and `close_write` (022.8).
- `create_dir(path)` creates the directory and any missing parent directories
  (022.9).
- `rename(src, dst)` moves `src` to `dst` only when `dst` does not exist, and
  fails when `dst` already exists (022.10, 022.11).
- `delete_file(path)` removes a file; `delete_dir(path)` removes an empty
  directory (022.12, 022.13).
- `set_mod_time(path, time)` sets the modification time of a file or directory
  (022.14).
- `list_dir` silently omits symbolic links, special files, and any other
  non-regular entry, and `stat` returns "not found" for a symbolic link or
  special file (022.15, 022.16).

## Boundaries

Error obligations:

- Every operation reports failure using only the categories not found, permission
  denied, and I/O error, regardless of whether the peer is `file://` or `sftp://`
  (022.17).
- A network failure such as a connection drop or timeout surfaces as an I/O
  error, never as a transport-specific error, so callers never match on scheme
  (022.18).
- An I/O failure produces the same sync handling whether it occurs on a `file://`
  peer or an `sftp://` peer (022.19).
- Transport returns these categorized errors and the unreachable verdict to its
  caller; it does not decide whether a failure skips a peer, retries a listing,
  or aborts the run. Those policy decisions belong to the components that drive
  the run.

Invariants:

- URL normalization is deterministic: the same input URL always produces the same
  canonical identity, and the worked examples in `003_url-normalization` hold
  exactly (003.11 through 003.16).
- After a peer's winning URL is selected, no other URL for that peer is tried
  again for the remainder of the run (005.4, 005.5).
- Both schemes behave identically across the boundary: the operation set, return
  shapes, omission rule, and error categories are the same for `file://` and
  `sftp://`, so identical contents yield identical results (022.1).
- `rename` never relies on rename-over-existing; a destination that already
  exists is a failure, leaving SWAP-style staged replacement to the callers that
  need it (022.11).

What Transport does not do:

- It does not recognize or split peer arguments on the command line; that parsing
  lives in `001_command-line`. Transport consumes already-separated URLs and
  normalizes them.
- It does not build the bounded-buffer streaming copy loop, retry copies, or do
  SWAP staging; it provides only the chunk-level read and write primitives that
  the copy-execution layer is built on.
- It does not decide canon, count reachable peers, or auto-subordinate; it only
  reports per-peer reachability and the winning URL.
- It does not own the dry-run decision globally; it still connects to peers
  exactly as a normal run does, and honors dry-run only where the connection
  rules require it, by not creating missing roots and treating an absent root as
  an unreachable URL for that run (024.1, 005.13, 005.14, 024.11).
- It does not emit progress or diagnostic output; connection and operation
  outcomes are returned to callers, which route any reporting through the output
  component.

Transport is a per-run singleton: a peer's winning URL and its live connection
are established once and reused by every component for the whole run, so a single
shared instance must hold that connection state. It depends on no sibling; it is
the foundational service the other components call, and it is marked shared so
descendants deeper in the tree may also reach peers through this one interface.

Construction and the hidden backends:

- Transport is split internally into three private helpers it owns and builds
  itself: the URL-normalization helper, the local `file://` backend, and the
  remote `sftp://` backend. These helpers are an implementation detail of
  Transport, not part of its public surface.
- The function that creates a Transport instance takes no parameters. It
  constructs its own URL-normalization helper, local backend, and SFTP backend
  internally; a caller hands it nothing and never names, imports, or constructs
  any of those helpers. Every value Transport accepts (a URL, a peer handle, a
  read or write handle, a path, a timeout, a `dry_run` flag) and every value it
  returns is one of Transport's own public types or a plain standard-library
  type. No parameter or return type of any public Transport operation, and no
  parameter of its constructor, is a type that belongs to the URL-normalization,
  local-backend, or SFTP-backend helper. Those helper types stay entirely behind
  the Transport boundary.
