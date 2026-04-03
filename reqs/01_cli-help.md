# CLI and Help Screen

Command-line argument parsing, validation, and help display.

## $REQ_CLI_001: Help on No Arguments
**Source:** ./specs/help.md (Section: top)

Running `kitchensync` with no arguments prints the help text to stdout and exits 0.

## $REQ_CLI_002: Help Flags
**Source:** ./specs/help.md (Section: top)

Running with `-h`, `--help`, or `/?` prints the help text to stdout and exits 0.

## $REQ_CLI_003: Help Text Content
**Source:** ./specs/help.md (Section: help text block)

The help text is embedded in the binary at build time and matches the verbatim text specified in specs/help.md, including usage line, peer formats, prefix modifiers, fallback URL syntax, per-URL settings, all options with defaults, quick start examples, and tips.

## $REQ_CLI_004: Argument Validation Errors
**Source:** ./specs/help.md (Section: top)

Argument validation errors (no peers, multiple `+` peers, unrecognized flags, invalid values) print a specific error message followed by the help text to stdout, and exit 1.

## $REQ_CLI_006: Option Value Validation
**Source:** ./specs/algorithm.md (Section: "Startup")

`--mc`, `--ct`, and `--si` must be positive integers (>= 1). `--xd`, `--bd`, and `--td` must be non-negative integers (>= 0, where 0 means "never"). `-vl` must be one of: error, warn, info, debug, trace. Invalid values cause an error message plus help text, exit 1.

## $REQ_CLI_007: Boolean Flags
**Source:** ./specs/algorithm.md (Section: "Startup")

`--dry-run` / `-n` and `--watch` are boolean flags that take no value.

## $REQ_CLI_008: Global Option Defaults
**Source:** ./README.md (Section: "Global Options")

Default values are: `--mc` 10, `--ct` 30, `-vl` info, `--xd` 2, `--bd` 90, `--td` 180, `--si` 30. `--dry-run` and `--watch` default to off.

## $REQ_CLI_009: Peer URL Formats
**Source:** ./README.md (Section: "URL Schemes")

The following peer URL forms are accepted: local paths (`/path`, `c:\path`, `./relative`), `sftp://user@host/path`, `sftp://user@host:port/path`, and `sftp://user:password@host/path`.

## $REQ_CLI_010: Prefix Modifiers on Peers
**Source:** ./README.md (Section: "The + and - URL Prefixes")

A `+` prefix marks a peer as canon (wins every disagreement). A `-` prefix marks a peer as subordinate (loses every disagreement). No prefix means bidirectional (newest wins).

## $REQ_CLI_011: Fallback URL Grouping
**Source:** ./README.md (Section: "Fallback URLs")

Multiple URLs for the same peer can be grouped with brackets: `[url1,url2,...]`. Prefix modifiers can be applied to the group: `+[url1,url2,...]` or `-[url1,url2,...]`.

## $REQ_CLI_012: Per-URL Settings
**Source:** ./README.md (Section: "Per-URL Tuning")

Individual URLs accept query-string parameters to override global options for that URL: `?mc=N` (max connections), `?ct=N` (connection timeout), or both `?mc=N&ct=N`.
