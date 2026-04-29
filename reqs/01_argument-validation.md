# 01_argument-validation: CLI argument and option validation

## Behavior

Argument and option validation runs at startup (`./specs/sync.md` — Startup step 1, and `./specs/help.md`). On any validation failure, the program prints a specific error message followed by the help text to stdout and exits 1. Validated conditions include the peer count, the number of `+` peers, recognized flags, and that all option values are valid types/ranges.

## $REQ_IDs
- `01.10` — Running with a single peer argument exits 1 (at least two peers required).
- `01.11` — Running with two peers each prefixed with `+` exits 1 (at most one canon peer per run).
- `01.12` — Running with an unrecognized flag exits 1.
- `01.13` — A non-positive-integer value for `--mc`, `--ct`, `--xd`, `--bd`, or `--td` exits 1.
- `01.14` — A `-vl` value not in `{error, info, debug, trace}` exits 1.
- `01.15` — Each validation-error invocation prints both an error message and the help text to stdout.

## Notes

Successful argument parsing of `--mc`/`--ct`/`--xd`/`--bd`/`--td`/`-vl` with valid values does not block startup; observable effects of these options are tested under their respective behavioral concerns (concurrency, retention, logging).
