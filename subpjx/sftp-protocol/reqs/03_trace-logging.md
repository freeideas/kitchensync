# 03_trace-logging: Trace log lines on pool acquire and release

## Behavior

When verbosity is set to `trace`, the transport emits one log line per `acquire` and one per `release`, naming the endpoint and the current in-use/`mc` counts so an operator can follow pool occupancy. Derived from `SPEC.md` §"Acquiring and releasing pooled connections" (final paragraph).

## $REQ_IDs
- `03.10` — When verbosity is set to `trace`, each `acquire` call emits one log line.
- `03.11` — When verbosity is set to `trace`, each `release` call emits one log line.
- `03.12` — Each such trace log line contains `endpoint=<user@host>` and `connections=<in_use>/<mc>`.
