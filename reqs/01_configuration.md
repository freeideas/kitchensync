# Configuration

Config file format, settings with defaults, peer definitions, URL schemes, and authentication.

## $REQ_CONFIG_001: JSON5 Config Format
**Source:** ./README.md (Section: "Quick Start")

The config file (`kitchensync-conf.json`) uses JSON5 format.

## $REQ_CONFIG_002: Peers Definition
**Source:** ./README.md (Section: "Quick Start")

The config file contains a `peers` object where each key is a peer name and each value contains a `urls` array.

## $REQ_CONFIG_003: Minimum Two Peers
**Source:** ./specs/sync.md (Section: "Startup")

The config must define at least two peers, validated at parse time.

## $REQ_CONFIG_004: Peer Name Format
**Source:** ./specs/help.md (Section: "Help Screen")

Peer names must match `[a-zA-Z0-9][a-zA-Z0-9_-]*` and be at most 64 characters.

## $REQ_CONFIG_005: Multiple URLs Per Peer
**Source:** ./README.md (Section: "Why KitchenSync?")

Each peer can have multiple URLs. They are tried in order; the first successful connection is used.

## $REQ_CONFIG_006: SFTP URL Scheme
**Source:** ./specs/help.md (Section: "Help Screen")

SFTP URLs follow the format `sftp://user@host/path`, with optional port (`sftp://user@host:port/path`) and optional inline password (`sftp://user:password@host/path`). SFTP paths are absolute from filesystem root.

## $REQ_CONFIG_007: File URL Scheme - Absolute
**Source:** ./specs/help.md (Section: "Help Screen")

`file:///absolute/path` specifies a local absolute path.

## $REQ_CONFIG_008: File URL Scheme - Relative
**Source:** ./specs/help.md (Section: "Help Screen")

`file://./relative/path` specifies a local path relative to the config file's directory.

## $REQ_CONFIG_009: Password Percent Encoding
**Source:** ./specs/help.md (Section: "Help Screen")

Special characters in URL passwords must be percent-encoded (`@` → `%40`, `:` → `%3A`).

## $REQ_CONFIG_010: Authentication Fallback Chain
**Source:** ./specs/help.md (Section: "Help Screen")

SFTP authentication uses a fallback chain, stopping at first success: (1) inline password from URL, (2) SSH agent (`SSH_AUTH_SOCK`), (3) `~/.ssh/id_ed25519`, (4) `~/.ssh/id_ecdsa`, (5) `~/.ssh/id_rsa`.

## $REQ_CONFIG_011: Host Key Verification
**Source:** ./specs/help.md (Section: "Help Screen")

Host keys are verified via `~/.ssh/known_hosts`. Unknown hosts are rejected.

## $REQ_CONFIG_012: Database Path Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `database` setting specifies the SQLite database path (default: `kitchensync.db`). If relative, it resolves from the config file's directory. If absolute, it is used as-is.

## $REQ_CONFIG_013: Connection Timeout Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `connection-timeout` setting specifies the number of seconds for SSH connect to be aborted (default: 30).

## $REQ_CONFIG_014: Max Connections Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `max-connections` setting specifies the maximum concurrent connections per peer (default: 10).

## $REQ_CONFIG_015: XFER Cleanup Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `xfer-cleanup-days` setting specifies when to delete stale staging dirs (default: 2 days).

## $REQ_CONFIG_016: Back Retention Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `back-retention-days` setting specifies when to delete displaced files (default: 90 days).

## $REQ_CONFIG_017: Tombstone Retention Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `tombstone-retention-days` setting specifies when to forget deletion records (default: 180 days).

## $REQ_CONFIG_018: Log Retention Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `log-retention-days` setting specifies when to purge log entries (default: 32 days).

## $REQ_CONFIG_019: Kitchensync Directory Not a Sync Target
**Source:** ./specs/help.md (Section: "Help Screen")

`.kitchensync/` can never be a sync target. Peer URL paths must not resolve to a `.kitchensync/` directory.
