# Help Screen

Display of usage information and argument validation error reporting.

## $REQ_HELP_001: Help on -h Flag
**Source:** ./specs/help.md (Section: "Help Screen")

Running the binary with `-h` prints the help text to stdout and exits 0.

## $REQ_HELP_002: Help on --help Flag
**Source:** ./specs/help.md (Section: "Help Screen")

Running the binary with `--help` prints the help text to stdout and exits 0.

## $REQ_HELP_003: Help on /? Flag
**Source:** ./specs/help.md (Section: "Help Screen")

Running the binary with `/?` prints the help text to stdout and exits 0.

## $REQ_HELP_004: Help on No Arguments
**Source:** ./specs/help.md (Section: "Help Screen")

Running the binary with no arguments prints the help text to stdout and exits 0.

## $REQ_HELP_005: Help Text Content
**Source:** ./specs/help.md (Section: "Help Screen")

The help text matches the verbatim content specified in help.md, including the usage line, peer formats, prefix modifiers, fallback URL syntax, per-URL settings, options table with defaults, quick start examples, and the closing note about displaced files.

## $REQ_HELP_006: Argument Validation Error Format
**Source:** ./specs/help.md (Section: "Help Screen")

When an argument validation error occurs (fewer than two peers, multiple `+` peers, unrecognized flags, invalid values), the binary prints a specific error message followed by the help text, and exits 1.
