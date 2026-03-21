# Configuration

Config directory resolution, config file format, CLI argument parsing, and settings management.

## $REQ_CFG_001: Default Config Directory
**Source:** ./specs/sync.md (Section: "Config Directory")

The default config directory is `~/.kitchensync/`.

## $REQ_CFG_002: Config Directory Creation
**Source:** ./specs/sync.md (Section: "Startup")

If the config directory does not exist, it is created on startup.

## $REQ_CFG_003: Cfg Flag With Path Ending in .kitchensync
**Source:** ./specs/sync.md (Section: "Command Line")

If `--cfgdir <path>` is specified and `<path>` ends with `.kitchensync/` or `.kitchensync`, it is used as-is (with a trailing `/` added if absent).

## $REQ_CFG_004: Cfg Flag With Other Path
**Source:** ./specs/sync.md (Section: "Command Line")

If `--cfgdir <path>` is specified and `<path>` does not end with `.kitchensync/` or `.kitchensync`, then `.kitchensync/` is appended to `<path>`.

## $REQ_CFG_005: Cfg Flag Without Path
**Source:** ./specs/sync.md (Section: "Command Line")

`--cfgdir` alone with no path uses the default `~/.kitchensync/`.

## $REQ_CFG_006: Config File Name
**Source:** ./specs/sync.md (Section: "Config Directory")

The config file is named `kitchensync-conf.json` inside the config directory. This filename is not configurable.

## $REQ_CFG_007: Config File JSON with Comments
**Source:** ./specs/database.md (Section: "Config file structure")

The config file is JSON with `//` and `/* */` comments allowed. Comments are stripped before parsing.

## $REQ_CFG_008: CLI URL Arguments
**Source:** ./specs/sync.md (Section: "Command Line")

Arguments without `=` are treated as peer URLs. Bare paths (no `file://` prefix) are treated as local `file://` URLs.

## $REQ_CFG_009: CLI Setting Arguments
**Source:** ./specs/sync.md (Section: "Command Line")

Arguments with `=` are treated as settings (key=value pairs) that apply to the current run and are persisted to the config file.

## $REQ_CFG_010: Canon Suffix on CLI
**Source:** ./specs/sync.md (Section: "Canon Peer")

A trailing `!` on a CLI URL marks that peer as canon for this run only. The `!` is not persisted to the config file.

## $REQ_CFG_011: Config File Accumulation
**Source:** ./specs/sync.md (Section: "Peer Groups")

The config file accumulates state across runs. CLI URLs, peers, and settings are merged into the config file and persisted.

## $REQ_CFG_012: Settings Defaults
**Source:** ./README.md (Section: "Settings")

Settings have the following defaults: `max-connections`=10, `connection-timeout`=30, `log-level`=info, `xfer-cleanup-days`=2, `back-retention-days`=90, `tombstone-retention-days`=180, `log-retention-days`=32.

## $REQ_CFG_013: Settings Persistence
**Source:** ./README.md (Section: "Settings")

Settings specified on the CLI as `key=value` are merged into the config file and persisted. The next run inherits them even if not specified again.

## $REQ_CFG_014: Config File Written After Successful Reconciliation
**Source:** ./specs/sync.md (Section: "Startup")

The merged config file is written only after peer identity reconciliation succeeds. If reconciliation fails, the original config file is unchanged.
