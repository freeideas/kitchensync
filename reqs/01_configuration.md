# Configuration

Config file format, settings, defaults, and peer definitions.

## $REQ_CONF_001: JSON5 Config Format
**Source:** ./README.md (Section: "Quick Start")

The config file (`kitchensync-conf.json`) uses JSON5 format.

## $REQ_CONF_002: Peers Section Required
**Source:** ./specs/help.md (Section: "Help Screen")

The config must contain a `peers` section. At least two peers are required.

## $REQ_CONF_003: Peer URLs Array
**Source:** ./specs/help.md (Section: "Help Screen")

Each peer has a `urls` array of one or more URLs. URLs are tried top-to-bottom; the first successful connection wins.

## $REQ_CONF_004: Database Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `database` setting specifies the SQLite database path. Default: `"kitchensync.db"`.

## $REQ_CONF_005: Connection Timeout Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `connection-timeout` setting specifies seconds before an SSH connection attempt is aborted. Default: 30.

## $REQ_CONF_006: Workers Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `workers` setting specifies the number of concurrent file copy threads. Default: 10.

## $REQ_CONF_007: XFER Cleanup Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `xfer-cleanup-days` setting specifies days before stale staging directories are deleted. Default: 2.

## $REQ_CONF_008: BACK Retention Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `back-retention-days` setting specifies days before displaced files are deleted. Default: 90.

## $REQ_CONF_009: Tombstone Retention Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `tombstone-retention-days` setting specifies days before deletion records are forgotten. Default: 180.

## $REQ_CONF_010: Log Retention Days Setting
**Source:** ./specs/help.md (Section: "Help Screen")

The `log-retention-days` setting specifies days before log entries are purged. Default: 32.

## $REQ_CONF_011: Relative Paths Resolve From Config Directory
**Source:** ./specs/help.md (Section: "Help Screen")

All relative paths in the config resolve from the config file's directory.

## $REQ_CONF_012: Peer URL .kitchensync Adjustment
**Source:** ./specs/help.md (Section: "Help Screen")

If the config file is inside a `.kitchensync/` directory, peer URL paths back up to the parent of `.kitchensync/` (so `"."` becomes `".."`). This adjustment applies only to peer URLs, not to other settings like `"database"`.

## $REQ_CONF_013: Peer Name Format
**Source:** ./specs/help.md (Section: "Help Screen")

Peer names must match `[a-zA-Z0-9][a-zA-Z0-9_-]*`, maximum 64 characters.
