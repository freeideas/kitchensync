# 01_arg-validation: CLI argument validation errors

## Behavior

Invalid command-line arguments cause KitchenSync to print an error message followed by the help text and exit 1. All output goes to stdout. Derived from `specs/help.md` and `specs/sync.md` §"Startup" step 1.

## $REQ_IDs
- `01.7` — Fewer than two peer arguments prints an error message followed by the help text on stdout and exits 1.
- `01.8` — More than one `+`-prefixed peer prints an error message followed by the help text on stdout and exits 1.
- `01.9` — An unrecognized flag prints an error message followed by the help text on stdout and exits 1.
- `01.10` — A non-positive-integer value for `--mc`, `--ct`, `--ka`, `--xd`, `--bd`, or `--td` prints an error message followed by the help text on stdout and exits 1.
- `01.11` — A `-vl` value not in {`error`, `info`, `debug`, `trace`} prints an error message followed by the help text on stdout and exits 1.

## Notes
The "first sync needs `+`" check happens after snapshot download, not during arg validation — see `02_startup-connect.md`.
