# 03_logging: Log output and verbosity levels

## Behavior

Every file copy and every displacement is logged once per decision (not once per peer) at `info` level with a short format. All output goes to stdout. Verbosity levels are cumulative: `error` < `info` < `debug` < `trace`. Trace-level adds pool acquire/release events. Derived from `sync.md` §Logging, `concurrency.md` §"Trace Logging", and `multi-tree-sync.md` §"Listing errors".

## $REQ_IDs

- `03.78` — Each file copy decision produces one log line of the form `C <relative-path>` at `info` verbosity, regardless of how many destination peers receive that file.
- `03.79` — Each displacement decision produces one log line of the form `X <relative-path>` at `info` verbosity, regardless of how many peers are affected.
- `03.80` — All log output goes to stdout.
- `03.90` — Stderr is empty during a sync run.
- `03.81` — `C` and `X` progress lines do not appear in the output at `-vl error`.
- `03.82` — Pool acquire and release events appear in the output at `-vl trace`.
- `03.83` — Pool acquire and release events do not appear in the output at `-vl error`, `-vl info`, or `-vl debug`.
- `03.84` — Pool acquire and release events use the format `endpoint=<user@host> connections=<n>/<max>` keyed by user+host.
- `03.85` — A `list_dir` failure on one peer at one directory produces a log line at `error` verbosity that identifies the affected peer and directory.

## Notes

The C/X lines log the *decision*, so the line appears even though the actual copy is enqueued and may complete later. Failed copies are logged separately at `error`.
