# 04_keepalive: Released connections stay reusable for `ka` seconds, then are closed

## Behavior
When a connection is released to a live pool, it joins the idle set with a fresh `ka` timer. If another `acquire` claims it before the timer expires, the timer is cancelled and the connection is handed back without invoking `open`. If `ka` elapses with no acquire, the pool invokes `close` on the idle connection. Derives from `./specs/SPEC.md` §"Acquiring and releasing".

## $REQ_IDs
- `04.1` — A connection released to a live pool is reusable on a subsequent `acquire` issued within `ka` seconds of the release.
- `04.2` — When `ka` seconds elapse after a release without any intervening `acquire`, the pool invokes `close` on the idle connection.
- `04.3` — When an `acquire` reuses an idle connection, that connection's `ka` timer is cancelled (the connection is not subsequently closed by the timer).

## Notes
The `ka` timer is per-release: each `release` starts a fresh window, even if the connection was previously idle and re-acquired.
