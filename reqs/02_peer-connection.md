# Peer Connection

URL schemes, authentication, connection handling, and filesystem abstraction.

## $REQ_PEER_001: SFTP URL Scheme
**Source:** ./specs/help.md (Section: "Help Screen")

`sftp://user@host/path` connects to a remote peer over SSH (port 22). SFTP paths are absolute from the filesystem root.

## $REQ_PEER_002: SFTP Non-Standard Port
**Source:** ./specs/help.md (Section: "Help Screen")

`sftp://user@host:port/path` connects using a non-standard SSH port.

## $REQ_PEER_003: SFTP Inline Password
**Source:** ./specs/help.md (Section: "Help Screen")

`sftp://user:password@host/path` uses an inline password. Special characters in passwords are percent-encoded (`@` → `%40`, `:` → `%3A`).

## $REQ_PEER_004: File URL Absolute Path
**Source:** ./specs/help.md (Section: "Help Screen")

`file:///absolute/path` accesses a local absolute path.

## $REQ_PEER_005: File URL Relative Path
**Source:** ./specs/help.md (Section: "Help Screen")

`file://./relative/path` accesses a local path relative to the config file's directory.

## $REQ_PEER_006: Multiple URLs Per Peer
**Source:** ./README.md (Section: "Why KitchenSync?")

Each peer can have multiple URLs. They are tried in order; the first successful connection is used.

## $REQ_PEER_007: Authentication Fallback Chain
**Source:** ./specs/help.md (Section: "Help Screen")

SFTP authentication uses a fallback chain, stopping at first success: (1) inline password from URL, (2) SSH agent (`SSH_AUTH_SOCK`), (3) `~/.ssh/id_ed25519`, (4) `~/.ssh/id_ecdsa`, (5) `~/.ssh/id_rsa`.

## $REQ_PEER_008: Host Key Verification
**Source:** ./specs/help.md (Section: "Help Screen")

Host keys are verified via `~/.ssh/known_hosts`. Unknown hosts are rejected.

## $REQ_PEER_009: Parallel Peer Connection
**Source:** ./specs/sync.md (Section: "Startup")

All peers are connected in parallel at startup.

## $REQ_PEER_016: Connection Timeout
**Source:** ./specs/help.md (Section: "Help Screen")

SSH connections are aborted after `connection-timeout` seconds (default: 30).

## $REQ_PEER_017: Peer Name Validation
**Source:** ./specs/help.md (Section: "Help Screen")

Peer names must match `[a-zA-Z0-9][a-zA-Z0-9_-]*` and be at most 64 characters.

## $REQ_PEER_010: Unreachable Peers Skipped
**Source:** ./specs/sync.md (Section: "Errors")

Unreachable peers are skipped with a warning logged. Sync continues with remaining peers.

## $REQ_PEER_011: Canon Peer Unreachable Exits With Error
**Source:** ./specs/sync.md (Section: "Startup")

If the `--canon` peer is unreachable, exit with an error.

## $REQ_PEER_012: Minimum Reachable Peers
**Source:** ./specs/sync.md (Section: "Startup")

At least two reachable peers are required. With `--canon`, one reachable peer (the canon peer itself) is sufficient.

## $REQ_PEER_013: Filesystem Abstraction
**Source:** ./specs/sync.md (Section: "Peer Filesystem Abstraction")

All sync logic operates through a single trait (interface) that both `file://` and `sftp://` implement. No protocol-specific code exists outside the trait implementations.

## $REQ_PEER_015: Filesystem Trait Required Operations
**Source:** ./specs/sync.md (Section: "Peer Filesystem Abstraction")

Both `file://` and `sftp://` implementations support: `list_dir` (children with name, is_dir, mod_time, byte_size), `stat` (mod_time, byte_size, is_dir, or not found), `read_file` (streaming read), `write_file` (create/overwrite from stream, creating parent dirs as needed), `rename` (same-filesystem), `delete_file`, `create_dir` (with parents as needed), and `delete_dir` (empty directory).

## $REQ_PEER_014: Uniform Error Types Across Transports
**Source:** ./specs/sync.md (Section: "Peer Filesystem Abstraction")

All filesystem operations return the same error types regardless of transport: not found, permission denied, I/O error. The sync logic never matches on transport-specific errors. Network failures surface as I/O errors.
