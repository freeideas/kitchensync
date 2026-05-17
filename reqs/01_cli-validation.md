# 01_cli-validation: Command-line argument validation

## Behavior

The program validates command-line arguments before doing any sync work on non-help invocations. Invalid arguments - exactly one peer, multiple canon peers, unrecognized flags, or invalid option values - must produce a validation error, then print the help text, and exit 1. Derived from `sync.md` Startup and Errors sections, and `help.md`. (Zero arguments is a help invocation, not a validation error; see `01_help-text` req 01.1.)

## $REQ_IDs

- `01.10` - Exactly one peer on the command line is an error: the program prints a validation error, then prints the help text, and exits 1. (Zero arguments is a help invocation; see `01_help-text` req 01.1.)
- `01.11` - More than one `+` (canon) peer is an error: the program prints a validation error, then prints the help text, and exits 1.
- `01.12` - Unrecognized flags are an error: the program prints a validation error, then prints the help text, and exits 1.
- `01.13` - Non-positive-integer values for `--mc`, `--ct`, `--ka`, `--xd`, `--bd`, or `--td` are an error: the program prints a validation error, then prints the help text, and exits 1.
- `01.14` - A `-vl` value outside `error`/`info`/`debug`/`trace` is an error: the program prints a validation error, then prints the help text, and exits 1.

## Notes

A first run with two or more peers but no canon and no snapshots produces a different message (`First sync? Mark the authoritative peer with a leading +`) and is covered by the first-sync requirements.
