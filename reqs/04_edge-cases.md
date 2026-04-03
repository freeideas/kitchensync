# Edge Cases

Case sensitivity, Unicode normalization, authentication, offline peers, and error handling.

## $REQ_EDGE_001: Case Collision Handling
**Source:** ./specs/algorithm.md (Section: "Case Sensitivity")

Filenames are preserved exactly as the filesystem reports them. On case-insensitive filesystems, multiple files differing only in case result in the last one (lexicographic order) overwriting earlier ones. A warning is logged. Displaced files are recoverable from BAK/.

## $REQ_EDGE_002: Case Collision Snapshot
**Source:** ./specs/algorithm.md (Section: "Case Sensitivity")

The destination snapshot records only the winning filename (last lexicographically). Source peer snapshots are unaffected.

## $REQ_EDGE_003: Unicode Byte-For-Byte Comparison
**Source:** ./specs/algorithm.md (Section: "Unicode Normalization")

Filenames are compared byte-for-byte as reported by the filesystem. No Unicode normalization is performed.

## $REQ_EDGE_004: SSH Authentication Order
**Source:** ./README.md (Section: "Authentication")

For remote peers, authentication is attempted in order: inline password from URL, SSH agent (`SSH_AUTH_SOCK`), `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, `~/.ssh/id_rsa`.

## $REQ_EDGE_005: Known Hosts Verification
**Source:** ./README.md (Section: "Authentication")

Host keys are verified via `~/.ssh/known_hosts`. Unknown hosts are rejected.

## $REQ_EDGE_006: Offline Peers Non-Fatal
**Source:** ./specs/algorithm.md (Section: "Offline Peers")

Unreachable peers are excluded entirely. Failure to connect to one peer is non-fatal -- exit 0 if at least one sync completes, or if single-peer snapshot completes.

## $REQ_EDGE_007: Transfer Failure Skips File
**Source:** ./specs/algorithm.md (Section: "Errors")

On transfer failure, the error is logged, TMP staging is cleaned up, and the file is skipped. It will be re-discovered on the next run.

## $REQ_EDGE_008: Displacement Failure Handling
**Source:** ./specs/algorithm.md (Section: "Errors")

On displacement failure: log error, skip (file remains). For directories: exclude the peer from recursion and do not cascade tombstones. The snapshot is left unchanged so the next run re-attempts.

## $REQ_EDGE_009: Peer Filesystem Interface
**Source:** ./specs/algorithm.md (Section: "Peer Filesystem Interface")

All sync logic operates through a single interface that both `file://` and `sftp://` implement. `ListDir` returns only regular files and directories -- symbolic links, special files, and non-regular entries are silently omitted.

## $REQ_EDGE_010: SFTP Uses OS Hostname Resolution
**Source:** ./specs/concurrency.md (Section: "Connection Establishment")

SFTP connections must use OS hostname resolution (e.g., Go's `net.Dial`). Bare hostnames like `localhost` must resolve correctly.

## $REQ_EDGE_011: Snapshot Upload Failure
**Source:** ./specs/algorithm.md (Section: "Errors")

On snapshot upload failure, the error is logged and the TMP staging file is left in place for `--xd` cleanup.

## $REQ_EDGE_011: Snapshot Upload Failure
**Source:** ./specs/algorithm.md (Section: "Errors")

On snapshot upload failure, the error is logged and the TMP staging file is left in place for `--xd` cleanup.
