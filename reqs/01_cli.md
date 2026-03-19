# Command Line Interface

Parsing of command-line arguments, config file resolution, and help output.

## $REQ_CLI_001: Config Argument Required
**Source:** ./specs/sync.md (Section: "Command Line")

The binary accepts a `<config>` positional argument: a path to a config file, a `.kitchensync/` directory, or a parent directory containing `.kitchensync/`.

## $REQ_CLI_002: Config Resolution - JSON File
**Source:** ./specs/help.md (Section: "Help Screen")

When `<config>` is a path to a `.json` file, it is used directly as the config file.

## $REQ_CLI_003: Config Resolution - .kitchensync Directory
**Source:** ./specs/help.md (Section: "Help Screen")

When `<config>` is a path to a `.kitchensync/` directory, `kitchensync-conf.json` is appended to form the config file path.

## $REQ_CLI_004: Config Resolution - Parent Directory
**Source:** ./specs/help.md (Section: "Help Screen")

When `<config>` is a path to any other directory, `.kitchensync/kitchensync-conf.json` is appended to form the config file path.

## $REQ_CLI_005: Canon Flag
**Source:** ./specs/sync.md (Section: "Command Line")

`--canon <peer-name>` makes the named peer authoritative for all decisions.

## $REQ_CLI_006: Help Flag Prints Help and Exits 0
**Source:** ./specs/help.md (Section: "Help Screen")

`-h` or `--help` prints the help text verbatim to stdout and exits 0.

## $REQ_CLI_007: Config Errors Printed and Exit
**Source:** ./specs/sync.md (Section: "Errors")

Configuration errors (bad JSON5, unknown peer in `--canon`, missing file) are printed to stdout and cause immediate exit.
