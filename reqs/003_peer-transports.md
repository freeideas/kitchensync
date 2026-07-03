# 003_peer-transports: Peer transport operations

## Behavior
This concern derives from `specs/sync.md` sections "Authentication (fallback
chain)", "Peer Transports", "Required Operations", "Error Semantics", "Case
Sensitivity", and "Testability", `specs/concurrency.md` section "Connection
Establishment", and `extart/ephemeral-sftp-server.py`. It covers the observable
local filesystem and SFTP peer operation surface, SSH authentication order,
known-host rejection of unknown hosts, remote and local root access,
transport-neutral error categories, omission of symbolic links and special
files from listings and stats, and preservation of reported filenames.

## $REQ_IDs
- `003.1` -- `file://` peer URLs and bare path peer URLs access peer files through local filesystem operations.
- `003.2` -- `sftp://` peer URLs access peer files through SSH/SFTP operations.
- `003.3` -- Local filesystem peers and SFTP peers provide the same required peer operation behavior to the sync engine.
- `003.4` -- After startup, peer operations use the connected root for the winning URL and paths relative to that root.
- `003.5` -- SFTP authentication tries the inline URL password, the SSH agent from `SSH_AUTH_SOCK`, `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, and `~/.ssh/id_rsa` in that order, continuing after each absent or rejected credential source.
- `003.6` -- An SFTP peer that accepts only the public key for `~/.ssh/id_ed25519` is reachable without an inline password, without a usable SSH agent, and without an accepted `~/.ssh/id_rsa`.
- `003.7` -- An SFTP URL whose presented host key matches `~/.ssh/known_hosts` is eligible for authentication.
- `003.8` -- An SFTP URL whose presented host key is unknown to `~/.ssh/known_hosts` is rejected.
- `003.9` -- Peer connection establishment tries the primary URL before that peer's fallback URLs.
- `003.10` -- Peer connection establishment tries a peer's fallback URLs in their listed order.
- `003.11` -- For SFTP URLs, the SSH handshake is bounded by `--timeout-conn` or the URL's `timeout-conn` parameter.
- `003.12` -- An SFTP URL whose SSH handshake exceeds its connection timeout is treated as failed for that run.
- `003.13` -- In a normal run, a successful SFTP connection creates the peer's missing remote root path and any missing parents before the URL wins.
- `003.14` -- In `--dry-run`, a successful SFTP connection with a missing remote root path does not create that path and treats that URL as failed for that run.
- `003.15` -- A successful SFTP connection whose remote root path cannot be created is treated as failed for that run.
- `003.16` -- Connection timeout and keep-alive settings do not apply to `file://` peer URLs.
- `003.17` -- In a normal run, a `file://` peer URL creates its missing local root path and any missing parents before the URL wins.
- `003.18` -- In `--dry-run`, a `file://` peer URL with a missing local root path does not create that path and treats that URL as failed for that run.
- `003.19` -- The first successful URL for a peer wins connection establishment.
- `003.20` -- After a URL wins connection establishment, remaining URLs for that peer are not tried in that run.
- `003.21` -- After startup, later peer operation failures do not cause the peer to switch to a fallback URL during the same run.
- `003.22` -- If every URL for a peer fails connection establishment, that peer is unreachable for the run.
- `003.23` -- `list_dir(peer, path)` returns only immediate child entries of `path`.
- `003.24` -- Each `list_dir(peer, path)` entry reports the child name, `is_dir`, `mod_time`, and `byte_size`.
- `003.25` -- `list_dir(peer, path)` reports `byte_size` as the file size in bytes for files.
- `003.26` -- `list_dir(peer, path)` reports `byte_size` as `-1` for directories.
- `003.27` -- `list_dir(peer, path)` omits symbolic links, special files, and all other non-regular entry types.
- `003.28` -- `stat(peer, path)` for a regular file or directory returns `mod_time`, `byte_size`, and `is_dir`.
- `003.29` -- `stat(peer, path)` for a missing path returns `not found`.
- `003.30` -- `stat(peer, path)` for a symbolic link, special file, or other non-regular entry type returns `not found`.
- `003.31` -- `open_read(peer, path)` opens a peer file for streaming read.
- `003.32` -- `read(handle, max_bytes)` returns the next byte chunk or EOF.
- `003.33` -- `close_read(handle)` closes an open peer read handle.
- `003.34` -- `open_write(peer, path)` creates the target file and any needed parent directories.
- `003.35` -- `write(handle, bytes)` stores the supplied byte chunk in the target file.
- `003.36` -- `close_write(handle)` finalizes the target file so later peer reads return the written bytes.
- `003.37` -- `rename(peer, src, dst)` moves `src` to a non-existing `dst` on the same filesystem.
- `003.38` -- `delete_file(peer, path)` removes the file at `path`.
- `003.39` -- `create_dir(peer, path)` creates the directory at `path` and any needed parents.
- `003.40` -- `delete_dir(peer, path)` removes the empty directory at `path`.
- `003.41` -- `set_mod_time(peer, path, time)` sets the modification time of the file or directory at `path`.
- `003.42` -- Peer operation failures are reported with the same `not found`, `permission denied`, and `I/O error` categories for local filesystem peers and SFTP peers.
- `003.43` -- SFTP connection drops and SFTP timeouts are reported as `I/O error`.
- `003.44` -- KitchenSync preserves filenames exactly as each peer filesystem reports them.

## Notes
This category is bounded by the transport API behavior visible to the sync
engine. Copy scheduling and replacement staging belong to
`008_copy-queue-and-concurrency` and `009_recoverable-staging`.
