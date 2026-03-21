# Help Screen

The binary prints a help screen and exits when invoked with `-h`, `--help`, or no arguments.

## $REQ_HELP_001: No Arguments Prints Help
**Source:** ./specs/help.md (Section: "Help Screen")

Running the binary with no arguments prints the help text to stdout and exits with code 0.

## $REQ_HELP_002: Help Flag -h
**Source:** ./specs/help.md (Section: "Help Screen")

Running the binary with `-h` prints the help text to stdout and exits with code 0.

## $REQ_HELP_003: Help Flag --help
**Source:** ./specs/help.md (Section: "Help Screen")

Running the binary with `--help` prints the help text to stdout and exits with code 0.

## $REQ_HELP_004: Help Text Content
**Source:** ./specs/help.md (Section: "Help Screen")

The help text printed is the verbatim text specified in specs/help.md, beginning with `Usage: kitchensync [--cfgdir [<path>]] <url>... [key=value...] [-h|--help]` and ending with `*"Canon" means source of truth; other peers will be made to look like the canon peer.`. The text includes sections for arguments, quick start examples, URL schemes, settings with defaults, peer group description, config directory description, and the canon footnote.
