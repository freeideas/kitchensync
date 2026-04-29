# 02_peer-connection: Connecting to peers, fallback URLs, root creation, reachability gating

## Behavior

At startup the program connects to all peers in parallel, trying each peer's URLs in fallback order until one succeeds and creating the root directory if it does not yet exist. Reachability of contributing and canon peers is enforced before any sync work runs. Derived from `./specs/sync.md` (`Startup`, `Errors`) and `./specs/concurrency.md` (`Connection Establishment`).

## $REQ_IDs
- `02.21` — When all peers are reachable, the run proceeds without the unreachable-peer warning being emitted.
- `02.22` — A `file://` peer whose root directory does not exist has the directory (and missing parents) auto-created at startup.
- `02.23` — An `sftp://` peer whose remote root path does not exist has it (and missing parents) auto-created over SFTP after the handshake succeeds.
- `02.24` — For a bracketed peer `[urlA,urlB]`, when `urlA` is unreachable and `urlB` is reachable, the run uses `urlB` and proceeds without aborting.
- `02.25` — When a peer (without fallbacks) is unreachable, the program logs a warning and continues with the remaining peers.
- `02.26` — When fewer than two peers end up reachable, the program exits with an error.
- `02.27` — When the canon peer (`+`) is unreachable, the program exits with an error.
- `02.28` — When all contributing (non-subordinate) peers are unreachable but at least one subordinate peer is reachable, the program exits with an error stating no contributing peer is reachable.
- `02.29` — When SFTP root creation fails on a URL, that URL is treated as failed and the next fallback URL is attempted.
