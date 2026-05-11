# 03_concurrency-bound: `mc` caps concurrency; `ct` bounds each open; failed opens don't consume slots

## Behavior
The setting `mc` caps how many connections one pool will hold concurrently. When `mc` connections are already in use and no idle connection is reusable, `acquire` blocks until capacity is freed by a `release` (or by an idle `ka` expiry). The setting `ct` bounds each `open` invocation; if `open` exceeds `ct` seconds or fails outright, the failure is surfaced to the caller of `acquire` and the slot is not consumed. Derives from `./specs/SPEC.md` §"Settings and connections" and §"Acquiring and releasing".

## $REQ_IDs
- `03.1` — With `mc=1` and one connection already in use, a second concurrent `acquire` does not return until the first connection is released.
- `03.2` — A blocked `acquire` proceeds and returns a connection once a `release` frees capacity.
- `03.3` — An `open` invocation that exceeds `ct` seconds is treated as a failed open.
- `03.4` — When `open` fails (by raising or by exceeding `ct`), `acquire` surfaces the failure to its caller.
- `03.5` — After a failed `open`, a subsequent `acquire` can still proceed up to `mc`.

## Notes
Slot accounting reflects only successful opens; a failed `open` does not retroactively occupy a slot.
