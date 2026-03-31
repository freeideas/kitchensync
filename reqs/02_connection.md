# Connection

Connection establishment, fallback URL handling, authentication, and peer root directory creation.

## $REQ_CONN_001: Parallel Peer Connection
**Source:** ./specs/algorithm.md (Section: "Startup")

At startup, connections to all peers are attempted in parallel.

## $REQ_CONN_002: Fallback URL Order
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For a peer with multiple fallback URLs, the primary URL is tried first, then each fallback in order. The first URL that successfully connects wins.

## $REQ_CONN_003: All Subsequent Connections Use Winning URL
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

After a fallback URL succeeds, all subsequent connections for that peer use the winning URL's pool.

## $REQ_CONN_004: SSH Handshake Timeout
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For SFTP URLs, the `--ct` option (default: 30 seconds) bounds the SSH handshake duration.

## $REQ_CONN_005: File URL No Timeout
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

For `file://` URLs, the connection timeout does not apply.

## $REQ_CONN_006: Auto-Create Peer Root Directory
**Source:** ./specs/algorithm.md (Section: "Startup")

Peer root directories are automatically created on connect, for both `file://` and `sftp://` peers.

## $REQ_CONN_007: Root Path Creation Failure Falls Back
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

After SSH handshake, the peer's root path is verified/created. If creation fails, the next fallback URL is tried.

## $REQ_CONN_008: OS Hostname Resolution
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

SFTP connections use OS hostname resolution (e.g., Go's `net.Dial`). Bare hostnames like `localhost` must resolve correctly.

## $REQ_CONN_009: Authentication Order
**Source:** ./README.md (Section: "Authentication")

For remote peers, authentication is attempted in this order: (1) inline password from URL, (2) SSH agent (`SSH_AUTH_SOCK`), (3) `~/.ssh/id_ed25519`, (4) `~/.ssh/id_ecdsa`, (5) `~/.ssh/id_rsa`.

## $REQ_CONN_010: Host Key Verification
**Source:** ./README.md (Section: "Authentication")

Host keys are verified via `~/.ssh/known_hosts`. Unknown hosts are rejected.

## $REQ_CONN_011: URL Normalization
**Source:** ./specs/database.md (Section: "URL Normalization")

URLs are normalized before any comparison, lookup, or connection attempt: lowercase scheme and hostname, remove default port (22 for SFTP), collapse consecutive slashes, remove trailing slash, convert bare paths to `file://`, resolve `file://` URLs to absolute path from cwd, percent-decode unreserved characters, strip query-string parameters.
