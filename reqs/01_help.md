# Help Screen

Displays help text when requested or when no arguments are provided.

## $REQ_HELP_001: No Arguments Prints Help
**Source:** ./specs/help.md (Section: "Help Screen")

Running `kitchensync` with no arguments prints the help text to stdout and exits 0.

## $REQ_HELP_002: Help Flag -h
**Source:** ./specs/help.md (Section: "Help Screen")

Running `kitchensync -h` prints the help text to stdout and exits 0.

## $REQ_HELP_003: Help Flag --help
**Source:** ./specs/help.md (Section: "Help Screen")

Running `kitchensync --help` prints the help text to stdout and exits 0.

## $REQ_HELP_004: Help Flag /?
**Source:** ./specs/help.md (Section: "Help Screen")

Running `kitchensync /?` prints the help text to stdout and exits 0.

## $REQ_HELP_005: Help Text Content
**Source:** ./specs/help.md (Section: "Help Screen")

The help text matches the verbatim content specified in `specs/help.md`, including the usage line, peer formats, prefix modifiers, fallback URL syntax, per-URL settings, all options with defaults, quick start examples, and tips.

## $REQ_HELP_006: Validation Error Shows Help
**Source:** ./specs/help.md (Section: "Help Screen")

Argument validation errors (no peers, multiple `+` peers, unrecognized flags, invalid values) print a specific error message followed by the help text, and exit 1.
