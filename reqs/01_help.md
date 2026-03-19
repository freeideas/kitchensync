# Help Screen

The binary prints a specific help text to stdout and exits when invoked with `-h` or `--help`.

## $REQ_HELP_001: Help Flag Short Form
**Source:** ./specs/build.md (Section: "Help Flag")

Passing `-h` prints the help text to stdout and exits with code 0.

## $REQ_HELP_002: Help Flag Long Form
**Source:** ./specs/build.md (Section: "Help Flag")

Passing `--help` prints the help text to stdout and exits with code 0.

## $REQ_HELP_003: Help Text Content
**Source:** ./specs/help.md (Section: "Help Screen")

The help output matches the text specified in `specs/help.md` verbatim.
