# 01_cli-validation: Command-line argument validation

## Behavior

The program validates command-line arguments before doing any sync work. Invalid arguments — too few peers, multiple canon peers, unrecognized flags, or invalid option values — cause the program to print an error message followed by the help text and exit 1. Derived from `sync.md` §Startup and §Errors, and `help.md`.

## $REQ_IDs

- `01.10` — Fewer than two peers on the command line is an error: prints an error message followed by help text and exits 1.
- `01.11` — More than one `+` (canon) peer is an error: prints an error message followed by help text and exits 1.
- `01.12` — Unrecognized flags are an error: prints an error message followed by help text and exits 1.
- `01.13` — Non-positive-integer values for `--mc`, `--ct`, `--ka`, `--xd`, `--bd`, or `--td` are an error: prints an error message followed by help text and exits 1.
- `01.14` — A `-vl` value outside `error`/`info`/`debug`/`trace` is an error: prints an error message followed by help text and exits 1.
- `01.15` — On any validation error, the help text follows the specific error message on stdout.
- `01.16` — Validation error output goes to stdout; stderr is empty.

## Notes

A first run with two or more peers but no canon and no snapshots produces a different message (`First sync? Mark the authoritative peer with a leading +`) and is covered by the first-sync requirements.
