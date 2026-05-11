# 02_logging: Progress and verbosity logging

## Behavior

KitchenSync logs progress to stdout. Each file copy and each deletion (displacement) produces one short line at `info` level — once per decision, not per peer pair. Pool acquire/release events log at `trace` level. Verbosity levels are cumulative. Derived from `specs/sync.md` §"Logging" and `specs/concurrency.md` §"Trace Logging".

## $REQ_IDs
- `02.35` — All log output goes to stdout.
- `02.36` — Each file copy decision emits a single `C <relative-path>` line at `info` level (one line per decision, not per peer pair).
- `02.37` — Each displacement decision emits a single `X <relative-path>` line at `info` level.
- `02.38` — Default verbosity is `info`: running without `-vl` produces the same output as `-vl info`.
- `02.39` — Verbosity levels are cumulative — each level emits everything lower levels emit plus its own additions.
- `02.40` — At `-vl error`, the `C` and `X` progress lines are absent from stdout.
- `02.41` — At `-vl trace`, every pool acquire and release emits a line `endpoint=<user@host> connections=<n>/<max>`.
- `02.42` — Pool acquire/release lines are absent from stdout at `-vl error`, `-vl info`, and `-vl debug`.
