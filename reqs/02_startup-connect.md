# 02_startup-connect: Connect to peers at startup

## Behavior

At startup, KitchenSync connects to all peers in parallel, auto-creates missing peer root directories, tries bracketed fallback URLs in order, and enforces minimum-reachability and canon checks before traversal begins. Derived from `specs/sync.md` §"Startup" and `specs/concurrency.md` §"Connection Establishment".

## $REQ_IDs
- `02.1` — Connections to all peers are issued concurrently, not sequentially.
- `02.2` — For a `file://` peer, a missing root directory (and any missing parents) is created before sync begins.
- `02.3` — For an `sftp://` peer, after the SSH handshake succeeds a missing remote root (and any missing parents) is created via SFTP; failure to create is treated as that URL failing.
- `02.4` — An unreachable peer is skipped with a warning, and sync continues with the remaining reachable peers.
- `02.5` — If fewer than two peers are reachable, the run exits with an error.
- `02.6` — If a `+`-prefixed canon peer is unreachable, the run exits with an error.
- `02.7` — For SFTP URLs, the `--ct` setting bounds the SSH handshake; on timeout, the URL is treated as failed and the next fallback URL is tried.
- `02.8` — On a first run where no peer has any snapshot history and no `+` canon peer is designated, KitchenSync prints `First sync? Mark the authoritative peer with a leading +` and exits 1.
- `02.9` — If no contributing (non-subordinate) peer is reachable after auto-subordination, the run exits with the error `No contributing peer reachable — cannot make sync decisions`.

## Notes
Fallback-URL ordering is covered by `03_fallback-urls.md`. Snapshot download mechanics are in `02_snapshot-download.md`.
