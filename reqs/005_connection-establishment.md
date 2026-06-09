# 005_connection-establishment: Per-peer connection and URL selection

## Behavior
This concern derives from `specs/concurrency.md` section "Connection
Establishment" and `specs/sync.md` section "Startup" step 2.

It covers how a single peer (possibly with bracketed fallback URLs) selects one
winning URL: trying the primary URL then each fallback in order, bounding the
SFTP handshake with `--timeout-conn` or the URL's `timeout-conn`, taking the
first URL that connects and not trying the rest, and treating all later
operations as bound to that winning URL. It also covers root-directory handling
per URL: in normal runs the peer root and any missing parents are auto-created
(for both `file://` and `sftp://`), and a URL whose root cannot be created is
treated as failed; in `--dry-run` missing roots are not created and such a URL
is treated as failed for that run. A peer with all URLs failing is unreachable.

The chain of credentials used for an SFTP handshake is `004_authentication`.
What happens to the reachable set after connection (counting peers, canon
checks, auto-subordination) is `006_run-lifecycle`. The global copy-slot meaning
of timeouts is `020_copy-execution`.

## $REQ_IDs

- `005.1` -- For a peer with multiple bracketed URLs, the primary URL is attempted before any fallback URL.
- `005.2` -- Fallback URLs are attempted in their listed order.
- `005.3` -- When an earlier URL fails to connect, the peer connects through the first later URL that connects.
- `005.4` -- The first URL that connects becomes the peer's winning URL, and no further URLs are attempted.
- `005.5` -- After a peer's winning URL is selected, the peer's other URLs are not used again for the remainder of the run.
- `005.6` -- For an `sftp://` URL, `--timeout-conn` bounds the SSH handshake.
- `005.7` -- A URL's `timeout-conn` query parameter overrides `--timeout-conn` for that URL's SSH handshake.
- `005.8` -- When an `sftp://` handshake does not complete within its connection timeout, that URL is abandoned and the next URL is attempted.
- `005.9` -- In a normal run, a missing peer root directory is created for a `file://` URL.
- `005.10` -- In a normal run, a missing peer root directory is created for an `sftp://` URL.
- `005.11` -- In a normal run, missing parent directories of the peer root are created.
- `005.12` -- In a normal run, a URL whose root directory cannot be created is treated as failed.
- `005.13` -- In `--dry-run`, a missing peer root directory is not created.
- `005.14` -- In `--dry-run`, a URL whose root directory does not already exist is treated as failed for that run.
- `005.15` -- A peer for which every URL fails is unreachable for the run.
