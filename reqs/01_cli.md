# Command Line Interface

Parsing of command-line arguments and resolution of the config file path.

## $REQ_CLI_001: Config Argument Required
**Source:** ./specs/sync.md (Section: "Command Line")

The binary accepts a positional `<config>` argument specifying the path to the config file, `.kitchensync/` directory, or parent directory.

## $REQ_CLI_002: Canon Flag
**Source:** ./specs/sync.md (Section: "Command Line")

The binary accepts an optional `--canon <peer-name>` flag that makes the named peer authoritative for all decisions.

## $REQ_CLI_003: Config Resolution - JSON File
**Source:** ./specs/help.md (Section: "Help Screen")

If `<config>` is a path to a `.json` file, it is used directly as the config file.

## $REQ_CLI_004: Config Resolution - Kitchensync Directory
**Source:** ./specs/help.md (Section: "Help Screen")

If `<config>` is a path to a `.kitchensync/` directory, `kitchensync-conf.json` is appended to form the config file path.

## $REQ_CLI_005: Config Resolution - Parent Directory
**Source:** ./specs/help.md (Section: "Help Screen")

If `<config>` is a path to any other directory, `.kitchensync/kitchensync-conf.json` is appended to form the config file path.

## $REQ_CLI_006: Relative Path Resolution
**Source:** ./specs/help.md (Section: "Help Screen")

All relative paths in the config file resolve from the config file's directory.

## $REQ_CLI_007: Peer URL Path Adjustment
**Source:** ./specs/help.md (Section: "Help Screen")

If the config file is inside a `.kitchensync/` directory, peer URL paths are adjusted to back up to the parent of `.kitchensync/` (so `file://./` in a config at `mydir/.kitchensync/kitchensync-conf.json` refers to `mydir/`). This adjustment applies only to peer URLs, not to other settings like `database`.
