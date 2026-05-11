#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises tombstone operations: mark-absent (03.8–03.11) and cascade-tombstone (03.12–03.14)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(tempfile.gettempdir())
DB_ABSENT = TMP / "snapshot_03_absent.db"
DB_CASCADE = TMP / "snapshot_03_cascade.db"


def _drain(stream):
    for _ in stream:
        pass


def _launch():
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
    )
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            break
    if port is None:
        proc.terminate()
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


_rpc_id = 0


def _rpc(sock, method, params=None):
    global _rpc_id
    _rpc_id += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def call(sock, tool, args=None):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": args or {}})
    if "error" in resp:
        raise RuntimeError(f"{tool} failed: {resp['error'].get('message', resp['error'])}")
    return resp.get("result", {})


def main() -> int:
    # Idempotency: remove any state from a previous run
    for db in (DB_ABSENT, DB_CASCADE):
        if db.exists():
            db.unlink()

    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # ── mark-absent tests (03.8–03.11) ──────────────────────────────
            ha = call(s, "open", {"path": str(DB_ABSENT)})["handle"]

            # 03.8 — mark-absent on a path with no row leaves snapshot unchanged
            call(s, "mark-absent", {"handle": ha, "path": "no/such/entry"})
            no_id = call(s, "hash-path", {"path": "no/such/entry"})["id"]
            row_08 = call(s, "lookup-row", {"handle": ha, "id": no_id}).get("row")
            print(f"[03.8] lookup after mark-absent on never-inserted path: {row_08!r}")
            if row_08 is not None:
                failures.append(f"03.8: expected no row for path with no snapshot entry, got {row_08!r}")

            # 03.9 + 03.10 — mark-absent sets deleted_time = last_seen, leaves last_seen unchanged
            ts_seen = call(s, "current-timestamp", {"handle": ha})["timestamp"]
            call(s, "upsert-present", {
                "handle": ha,
                "path": "f/a.txt",
                "basename": "a.txt",
                "mod_time": ts_seen,
                "byte_size": 42,
                "last_seen": ts_seen,
            })
            call(s, "mark-absent", {"handle": ha, "path": "f/a.txt"})
            aid = call(s, "hash-path", {"path": "f/a.txt"})["id"]
            row_a = call(s, "lookup-row", {"handle": ha, "id": aid}).get("row") or {}

            print(f"[03.9] deleted_time={row_a.get('deleted_time')!r}, want={ts_seen!r}")
            if row_a.get("deleted_time") != ts_seen:
                failures.append(
                    f"03.9: deleted_time={row_a.get('deleted_time')!r}, want {ts_seen!r}"
                )

            print(f"[03.10] last_seen={row_a.get('last_seen')!r}, want={ts_seen!r} (unchanged)")
            if row_a.get("last_seen") != ts_seen:
                failures.append(
                    f"03.10: last_seen changed to {row_a.get('last_seen')!r}, was {ts_seen!r}"
                )

            # 03.11 — repeated mark-absent is idempotent
            call(s, "mark-absent", {"handle": ha, "path": "f/a.txt"})
            row_a2 = call(s, "lookup-row", {"handle": ha, "id": aid}).get("row") or {}
            print(f"[03.11] row after second mark-absent: {row_a2!r}")
            if row_a2 != row_a:
                failures.append(f"03.11: row changed on second mark-absent: before={row_a!r}, after={row_a2!r}")

            call(s, "close", {"handle": ha})

            # ── cascade-tombstone tests (03.12–03.14) ────────────────────────
            hc = call(s, "open", {"path": str(DB_CASCADE)})["handle"]

            def stamp():
                return call(s, "current-timestamp", {"handle": hc})["timestamp"]

            # Tree:
            #   d/          dir (cascade target)
            #   d/live      file (live → should be tombstoned by cascade)
            #   d/dead      file (pre-tombstoned → cascade must not overwrite)
            #   d/live/g    file (grandchild, live → should be tombstoned)
            #   x/          dir (unrelated subtree)
            #   x/y         file (must not be touched)
            for path, size in [
                ("d", -1), ("d/live", 5), ("d/dead", 5), ("d/live/g", 3),
                ("x", -1), ("x/y", 7),
            ]:
                t = stamp()
                call(s, "upsert-present", {
                    "handle": hc,
                    "path": path,
                    "basename": path.rsplit("/", 1)[-1],
                    "mod_time": t,
                    "byte_size": size,
                    "last_seen": t,
                })

            # Pre-tombstone d/dead so it already has a deleted_time before cascade
            call(s, "mark-absent", {"handle": hc, "path": "d/dead"})
            dead_id = call(s, "hash-path", {"path": "d/dead"})["id"]
            dead_dt_pre = (call(s, "lookup-row", {"handle": hc, "id": dead_id}).get("row") or {}).get("deleted_time")

            # Run cascade-tombstone on d
            d_id = call(s, "hash-path", {"path": "d"})["id"]
            ts_c = stamp()
            call(s, "cascade-tombstone", {"handle": hc, "id": d_id, "timestamp": ts_c})

            # Fetch rows after cascade
            live_id = call(s, "hash-path", {"path": "d/live"})["id"]
            g_id = call(s, "hash-path", {"path": "d/live/g"})["id"]
            y_id = call(s, "hash-path", {"path": "x/y"})["id"]

            live_row = call(s, "lookup-row", {"handle": hc, "id": live_id}).get("row") or {}
            g_row = call(s, "lookup-row", {"handle": hc, "id": g_id}).get("row") or {}
            dead_row = call(s, "lookup-row", {"handle": hc, "id": dead_id}).get("row") or {}
            y_row = call(s, "lookup-row", {"handle": hc, "id": y_id}).get("row") or {}

            # 03.12 — live descendants get deleted_time = ts_c
            print(
                f"[03.12] d/live.deleted_time={live_row.get('deleted_time')!r},"
                f" d/live/g.deleted_time={g_row.get('deleted_time')!r}, want={ts_c!r}"
            )
            if live_row.get("deleted_time") != ts_c:
                failures.append(
                    f"03.12: d/live.deleted_time={live_row.get('deleted_time')!r}, want {ts_c!r}"
                )
            if g_row.get("deleted_time") != ts_c:
                failures.append(
                    f"03.12: d/live/g.deleted_time={g_row.get('deleted_time')!r}, want {ts_c!r}"
                )

            # 03.13 — already-tombstoned descendant's deleted_time is preserved
            dead_dt_post = dead_row.get("deleted_time")
            print(f"[03.13] d/dead deleted_time: pre={dead_dt_pre!r}, post={dead_dt_post!r}")
            if dead_dt_post != dead_dt_pre:
                failures.append(
                    f"03.13: cascade overwrote d/dead deleted_time:"
                    f" was {dead_dt_pre!r}, now {dead_dt_post!r}"
                )

            # 03.14 — unrelated x/y row is untouched
            print(f"[03.14] x/y.deleted_time={y_row.get('deleted_time')!r} (want None)")
            if y_row.get("deleted_time") is not None:
                failures.append(
                    f"03.14: x/y.deleted_time={y_row.get('deleted_time')!r}, should be None"
                )

            call(s, "close", {"handle": hc})

            if failures:
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            print("\nAll assertions passed.")
            return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
