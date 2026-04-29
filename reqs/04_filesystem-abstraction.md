# 04_filesystem-abstraction: Common interface for `file://` and `sftp://` peers

## Behavior

All sync logic — listing, copying, staging, displacing, cleaning, hashing — accesses peer filesystems through a single Java interface that both `file://` and `sftp://` implement. The interface returns the same error categories regardless of transport, and silently omits non-regular entries (symlinks, devices, FIFOs, sockets) from listings and stat lookups. Derived from `./specs/sync.md` (`Peer Filesystem Abstraction`, `Required Operations`, `Error Semantics`, `Why This Matters`) and `./specs/ignore.md` (`Symlinks`, `Built-in Excludes`).

## $REQ_IDs
- `04.31` — A `file://` peer and an `sftp://` peer with identical content sync to the same end-state when paired against the same other peer.
- `04.32` — `list_dir` on either transport returns name, is_dir, mod_time, and byte_size for each child; `byte_size = -1` is reported for directories.
- `04.33` — `list_dir` omits symbolic links, devices, FIFOs, and sockets from its results on either transport.
- `04.34` — `stat` returns `not found` for a path that is a symbolic link or special file on either transport.
- `04.35` — Network failures (SFTP channel drop, handshake timeout after connection) surface to sync logic as I/O errors — the same category produced by local I/O failures, not a transport-specific error type.
- `04.36` — A "not found" result from either transport surfaces as the same error category to sync logic.
- `04.37` — No source code outside the filesystem-interface implementations contains transport-specific code paths for `sftp://` versus `file://` (code-examination check on the sync logic).
