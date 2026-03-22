# Command-Line Interface

Parsing of command-line arguments including peers, prefixes, fallback URLs, per-URL settings, global options, URL schemes, and authentication.

## $REQ_CLI_025: Help Display
**Source:** ./specs/sync.md (Section: "Command Line")

No arguments, `-h`, `--help`, or `/?` prints help text and exits 0.

## $REQ_CLI_001: Minimum Two Peers Required
**Source:** ./specs/sync.md (Section: "Peers")

At least two peers are required on the command line. Fewer than two is a validation error.

## $REQ_CLI_002: Canon Peer Prefix
**Source:** ./specs/sync.md (Section: "Peers")

A `+` prefix on a peer marks it as the canon peer whose state wins all conflicts. Example: `+c:/photos`.

## $REQ_CLI_003: At Most One Canon Peer
**Source:** ./specs/sync.md (Section: "Peers")

At most one `+` peer is allowed per run. Multiple `+` peers is a validation error.

## $REQ_CLI_004: Subordinate Peer Prefix
**Source:** ./specs/sync.md (Section: "Peers")

A `-` prefix on a peer marks it as subordinate. It does not contribute to decisions but receives the group's outcome. Example: `-/mnt/usb/photos`.

## $REQ_CLI_005: Multiple Subordinate Peers Allowed
**Source:** ./specs/sync.md (Section: "Peers")

Multiple `-` peers are allowed in a single run.

## $REQ_CLI_006: Normal Bidirectional Peer
**Source:** ./specs/sync.md (Section: "Peers")

A peer with no prefix is a normal bidirectional peer that contributes to and receives sync decisions.

## $REQ_CLI_008: Local Path URL Schemes
**Source:** ./specs/sync.md (Section: "URL Schemes")

Local paths in the forms `/path`, `c:\path`, or `./relative` are supported and treated as `file://` URLs.

## $REQ_CLI_009: SFTP URL Scheme
**Source:** ./specs/sync.md (Section: "URL Schemes")

`sftp://user@host/path` connects to a remote peer over SSH on port 22.

## $REQ_CLI_010: SFTP Non-Standard Port
**Source:** ./specs/sync.md (Section: "URL Schemes")

`sftp://user@host:port/path` connects using a non-standard SSH port.

## $REQ_CLI_011: SFTP Inline Password
**Source:** ./specs/sync.md (Section: "URL Schemes")

`sftp://user:password@host/path` uses an inline password for authentication.

## $REQ_CLI_012: Percent-Encoding in SFTP Passwords
**Source:** ./specs/sync.md (Section: "URL Schemes")

Special characters in SFTP passwords are percent-encoded (`@` → `%40`, `:` → `%3A`).

## $REQ_CLI_013: Fallback URL Bracket Syntax
**Source:** ./specs/sync.md (Section: "Fallback URLs")

Square brackets group multiple URLs into a single peer representing different network paths to the same data. URLs inside are comma-separated and tried in order; the first that connects wins.

## $REQ_CLI_014: Prefix on Bracket Group
**Source:** ./specs/sync.md (Section: "Fallback URLs")

The `+`/`-` prefix goes on the bracket, not on individual URLs inside. Example: `+[sftp://host1/path,sftp://host2/path]`.

## $REQ_CLI_015: Per-URL Query String Settings
**Source:** ./specs/sync.md (Section: "Per-URL Settings")

Query-string parameters on a URL override global settings for that URL's connection. Supported parameters: `mc` (max connections), `ct` (connection timeout).

## $REQ_CLI_016: Global Option --mc
**Source:** ./specs/sync.md (Section: "Global Options")

`--mc N` sets the maximum concurrent connections per URL. Default: 10.

## $REQ_CLI_017: Global Option --ct
**Source:** ./specs/sync.md (Section: "Global Options")

`--ct N` sets the SSH handshake timeout in seconds. Default: 30.

## $REQ_CLI_018: Global Option -vl
**Source:** ./specs/sync.md (Section: "Global Options")

`-vl LEVEL` sets the verbosity level. Valid values: `error`, `info`, `debug`, `trace`. Default: `info`.

## $REQ_CLI_019: Global Option --xd
**Source:** ./specs/sync.md (Section: "Global Options")

`--xd N` sets the number of days after which stale TMP staging is deleted. Default: 2.

## $REQ_CLI_020: Global Option --bd
**Source:** ./specs/sync.md (Section: "Global Options")

`--bd N` sets the number of days after which displaced files in BAK/ are deleted. Default: 90.

## $REQ_CLI_021: Global Option --td
**Source:** ./specs/sync.md (Section: "Global Options")

`--td N` sets the number of days after which deletion records (tombstones) are forgotten. Default: 180.

## $REQ_CLI_022: Option Validation
**Source:** ./specs/sync.md (Section: "Startup")

`--mc` and `--ct` must be positive integers. `-vl` must be one of `error`/`info`/`debug`/`trace`. Invalid values are validation errors.

## $REQ_CLI_026: Unrecognized Flags Are Validation Errors
**Source:** ./specs/sync.md (Section: "Startup")

Unrecognized flags are validation errors.

## $REQ_CLI_027: Validation Error Behavior
**Source:** ./specs/sync.md (Section: "Startup")

On any validation error, print the error message followed by the help text and exit 1.

## $REQ_CLI_023: Authentication Fallback Chain
**Source:** ./specs/sync.md (Section: "Authentication (fallback chain)")

For SFTP connections, authentication is attempted in order: (1) inline password from URL, (2) SSH agent (`SSH_AUTH_SOCK`), (3) `~/.ssh/id_ed25519`, (4) `~/.ssh/id_ecdsa`, (5) `~/.ssh/id_rsa`.

## $REQ_CLI_024: Host Key Verification
**Source:** ./specs/sync.md (Section: "Authentication (fallback chain)")

Host keys are verified via `~/.ssh/known_hosts`. Unknown hosts are rejected.
