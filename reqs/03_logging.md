# 03_logging: Verbosity levels and progress output format

## Behavior

All program output goes to stdout. Each decided file copy and each decided deletion (displacement to BAK/) is logged once at `info` level in a short format. Verbosity levels (`error` < `info` < `debug` < `trace`) are cumulative; pool acquire/release events are logged only at `trace`. Derived from `./specs/sync.md` (`Logging`) and `./specs/concurrency.md` (`Trace Logging`).

## $REQ_IDs
- `03.81` — All program output is written to stdout (stderr is empty during a normal run).
- `03.82` — Each propagated file copy is logged at `info` level as a single line `C <relative-path>`.
- `03.83` — Each propagated deletion (displacement to BAK/) is logged at `info` level as a single line `X <relative-path>`.
- `03.84` — A copy or deletion is logged once per decision, not once per destination peer.
- `03.85` — At `-vl error`, `C` and `X` lines are not emitted.
- `03.86` — At `-vl info`, connection-pool acquire/release lines are not emitted.
- `03.87` — At `-vl trace`, every connection-pool acquire and release is logged in the format `url=<url> connections=<n>/<max>`.
- `03.88` — At `-vl debug`, no pool acquire/release lines appear (debug emits the same content as info until debug-only messages are defined).
