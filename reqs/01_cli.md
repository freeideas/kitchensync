# CLI Parsing

Command-line argument parsing: URL schemes, peer prefixes, fallback URLs, per-URL parameters, and global options.

## $REQ_CLI_001: Local Path URLs
**Source:** ./README.md (Section: "URL Schemes")

Local paths (`/path`, `c:\path`, `./relative`) are accepted as peer arguments and treated the same as `file://` URLs.

## $REQ_CLI_002: SFTP URL with User and Host
**Source:** ./README.md (Section: "URL Schemes")

`sftp://user@host/path` specifies a remote peer over SSH on port 22.

## $REQ_CLI_003: SFTP URL with Non-Standard Port
**Source:** ./README.md (Section: "URL Schemes")

`sftp://user@host:port/path` specifies a remote peer over SSH on a non-standard port.

## $REQ_CLI_004: SFTP URL with Inline Password
**Source:** ./README.md (Section: "URL Schemes")

`sftp://user:password@host/path` specifies a remote peer with an inline password.

## $REQ_CLI_005: Canon Prefix
**Source:** ./README.md (Section: "The `+` and `-` URL Prefixes")

A `+` prefix on a peer marks it as canon — this peer's state wins every disagreement.

## $REQ_CLI_006: Subordinate Prefix
**Source:** ./README.md (Section: "The `+` and `-` URL Prefixes")

A `-` prefix on a peer marks it as subordinate — this peer loses every disagreement.

## $REQ_CLI_007: No Prefix Bidirectional
**Source:** ./README.md (Section: "The `+` and `-` URL Prefixes")

A peer with no prefix participates bidirectionally; newest modification time wins.

## $REQ_CLI_008: Fallback URLs in Brackets
**Source:** ./README.md (Section: "Fallback URLs")

Multiple URLs for the same peer are grouped in square brackets, comma-separated: `[url1,url2,...]`. They are tried in order; first that connects wins.

## $REQ_CLI_009: Prefix on Bracket Group
**Source:** ./specs/concurrency.md (Section: "Fallback URLs")

The `+`/`-` prefix goes on the bracket group, not on individual URLs: `+[url1,url2,...]` or `-[url1,url2,...]`.

## $REQ_CLI_010: Per-URL Query Parameters
**Source:** ./README.md (Section: "Per-URL Tuning")

Per-URL settings are specified via query string parameters: `mc` for max connections, `ct` for connection timeout. Example: `"sftp://host/path?mc=5&ct=60"`.

## $REQ_CLI_011: Global Option --mc
**Source:** ./README.md (Section: "Global Options")

`--mc N` sets max concurrent connections per URL. Default: 10.

## $REQ_CLI_012: Global Option --ct
**Source:** ./README.md (Section: "Global Options")

`--ct N` sets SSH handshake timeout in seconds. Default: 30.

## $REQ_CLI_013: Global Option -vl
**Source:** ./README.md (Section: "Global Options")

`-vl LEVEL` sets verbosity level. Valid values: error, info, debug, trace. Default: info.

## $REQ_CLI_014: Global Option --xd
**Source:** ./README.md (Section: "Global Options")

`--xd N` sets stale TMP staging deletion after N days. 0 means never. Default: 2.

## $REQ_CLI_015: Global Option --bd
**Source:** ./README.md (Section: "Global Options")

`--bd N` sets displaced file (BAK/) deletion after N days. 0 means never. Default: 90.

## $REQ_CLI_016: Global Option --td
**Source:** ./README.md (Section: "Global Options")

`--td N` sets deletion record (tombstone) forgetting after N days. 0 means never. Default: 180.

## $REQ_CLI_017: Per-URL Overrides Global
**Source:** ./specs/concurrency.md (Section: "Connection Pool")

Per-URL settings (query string) override global settings for that URL.

## $REQ_CLI_018: At Most One Canon Peer
**Source:** ./specs/algorithm.md (Section: "Startup")

At most one peer may have the `+` (canon) prefix. Multiple `+` peers is a validation error.

## $REQ_CLI_019: Positive Integer Options
**Source:** ./specs/algorithm.md (Section: "Startup")

`--mc` and `--ct` values must be positive integers (>= 1). Invalid values are a validation error.

## $REQ_CLI_020: Non-Negative Integer Options
**Source:** ./specs/algorithm.md (Section: "Startup")

`--xd`, `--bd`, and `--td` values must be non-negative integers (>= 0). Invalid values are a validation error.

## $REQ_CLI_021: Valid Verbosity Levels
**Source:** ./specs/algorithm.md (Section: "Startup")

`-vl` must be one of: error, info, debug, trace. Other values are a validation error.

## $REQ_CLI_022: Help Display
**Source:** ./specs/help.md (Section: "Help Screen")

`-h`, `--help`, `/?`, or no arguments at all prints help text to stdout and exits 0.

## $REQ_CLI_023: Validation Error Output
**Source:** ./specs/help.md (Section: "Help Screen")

On any argument validation error, print the error message followed by help text to stdout and exit 1.

## $REQ_CLI_024: At Least One Peer Required
**Source:** ./specs/algorithm.md (Section: "Startup")

At least one peer URL must be provided. Missing peers is a validation error.
