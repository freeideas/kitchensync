# 02_acquire-release-basics: Acquire opens or reuses; release returns to idle

## Behavior
On a freshly registered pool, `acquire` invokes the caller-supplied `open` callback and returns the resulting `Connection`. `release` returns the connection to the pool's idle set, and a subsequent `acquire` (while the connection is still within its keep-alive window) hands the same idle connection back without invoking `open` again. Derives from `./specs/SPEC.md` §"Acquiring and releasing".

## $REQ_IDs
- `02.1` — `acquire` on a pool that has no idle connections and zero in-use connections invokes `open` exactly once and returns the connection that `open` produced.
- `02.2` — `release(pool, connection)` returns the connection to the pool's idle set (it does not invoke `close` immediately on a live pool).
- `02.3` — A subsequent `acquire` after `release` returns the same idle connection and does not invoke `open` again.

## Notes
The pool treats the `Connection` value opaquely — it is whatever `open` returned and is handed back unchanged. Reuse is conditioned on still being within the `ka` window; expiry behavior is covered in `04_keepalive.md`.
