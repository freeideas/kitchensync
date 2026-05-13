#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""purge_older_than deletes expired rows and retains rows inside the retention window."""

from __future__ import annotations

import itertools, json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = Path(os.environ.get("AITC_PROJECT", "."))
TMP = PROJECT / "tmp" / "testks" / "03-purge"

OLD_TS = "2026-01-01_00-00-00_000000Z"
RECENT_TS = "2026-05-10_00-00-00_000000Z"
NOW_TS = "2026-05-12_00-00-00_000000Z"
RETENTION_DAYS = 30


def _drain(stream):
    for _ in stream:
        pass


def _launch():
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", str(PROJECT)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
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


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
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


def _result(resp):
    if "error" in resp:
        return False, resp["error"].get("message")
    return True, resp.get("result")


def _clean_db(path):
    TMP.mkdir(parents=True, exist_ok=True)
    for suffix in ("", "-wal", "-shm"):
        Path(str(path) + suffix).unlink(missing_ok=True)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            ids = itertools.count(1)

            def call(tool, args):
                return _rpc(s, "tools/call", {"name": tool, "arguments": args}, next(ids))

            def open_db(name):
                db = TMP / name
                _clean_db(db)
                ok, result = _result(call("open", {"file": str(db)}))
                if not ok:
                    failures.append(f"{name}: open failed: {result}")
                    return None
                handle = result.get("handle") if isinstance(result, dict) else None
                if not handle:
                    failures.append(f"{name}: open did not return a handle: {result!r}")
                return handle

            def close_db(handle):
                if handle is not None:
                    call("close", {"handle": handle})

            def expect_ok(label, resp):
                ok, result = _result(resp)
                if not ok:
                    failures.append(f"{label}: tool call failed: {result}")
                return ok

            def lookup(label, handle, path):
                ok, result = _result(call("lookup", {"handle": handle, "path": path}))
                if not ok:
                    failures.append(f"{label}: lookup failed: {result}")
                    return None
                return (result or {}).get("record")

            # --- 03.1: delete tombstone row whose deleted_time is old ---
            print("[03.1] purge deletes tombstone rows with old deleted_time")
            h = open_db("031.db")
            if h is not None:
                expect_ok("03.1 setup upsert", call("upsert-observed", {
                    "handle": h, "path": "a/x.txt",
                    "mod_time": RECENT_TS, "byte_size": 10, "is_dir": False, "now": RECENT_TS,
                }))
                expect_ok("03.1 setup tombstone", call("mark-subtree-deleted", {
                    "handle": h, "path": "a/x.txt", "deleted_time": OLD_TS,
                }))
                before = lookup("03.1 before purge", h, "a/x.txt")
                if (
                    before is None
                    or before.get("deleted_time") != OLD_TS
                    or before.get("last_seen") != RECENT_TS
                ):
                    failures.append(
                        "03.1: precondition failed, row was not a tombstone "
                        f"with old deleted_time and recent last_seen: {before!r}"
                    )
                expect_ok("03.1 purge", call("purge-older-than", {
                    "handle": h, "retention_days": RETENTION_DAYS, "now": NOW_TS,
                }))
                after = lookup("03.1 after purge", h, "a/x.txt")
                print(
                    f"  before_deleted_time={before.get('deleted_time') if before else 'N/A'}, "
                    f"before_last_seen={before.get('last_seen') if before else 'N/A'}, "
                    f"row_after={after is not None}"
                )
                if (
                    before is not None
                    and before.get("deleted_time") == OLD_TS
                    and before.get("last_seen") == RECENT_TS
                    and after is not None
                ):
                    failures.append("03.1: old tombstone row was not deleted by purge")
            close_db(h)

            # --- 03.2: delete non-tombstone row whose last_seen is old ---
            print("[03.2] purge deletes non-tombstone rows with old last_seen")
            h = open_db("032.db")
            if h is not None:
                expect_ok("03.2 setup upsert", call("upsert-observed", {
                    "handle": h, "path": "a/y.txt",
                    "mod_time": OLD_TS, "byte_size": 20, "is_dir": False, "now": OLD_TS,
                }))
                before = lookup("03.2 before purge", h, "a/y.txt")
                if before is None or before.get("last_seen") != OLD_TS or before.get("deleted_time") is not None:
                    failures.append(f"03.2: precondition failed, row was not an old non-tombstone: {before!r}")
                expect_ok("03.2 purge", call("purge-older-than", {
                    "handle": h, "retention_days": RETENTION_DAYS, "now": NOW_TS,
                }))
                after = lookup("03.2 after purge", h, "a/y.txt")
                print(f"  before_last_seen={before.get('last_seen') if before else 'N/A'}, row_after={after is not None}")
                if (
                    before is not None
                    and before.get("last_seen") == OLD_TS
                    and before.get("deleted_time") is None
                    and after is not None
                ):
                    failures.append("03.2: non-tombstone row with old last_seen was not deleted by purge")
            close_db(h)

            # --- 03.3: delete non-tombstone row whose last_seen is null ---
            print("[03.3] purge deletes non-tombstone rows with null last_seen")
            h = open_db("033.db")
            if h is not None:
                expect_ok("03.3 setup record_decided", call("record-decided", {
                    "handle": h, "path": "a/z.txt",
                    "mod_time": OLD_TS, "byte_size": 30, "is_dir": False,
                }))
                before = lookup("03.3 before purge", h, "a/z.txt")
                if before is None or before.get("last_seen") is not None or before.get("deleted_time") is not None:
                    failures.append(f"03.3: precondition failed, row was not a non-tombstone with null last_seen: {before!r}")
                expect_ok("03.3 purge", call("purge-older-than", {
                    "handle": h, "retention_days": RETENTION_DAYS, "now": NOW_TS,
                }))
                after = lookup("03.3 after purge", h, "a/z.txt")
                print(f"  before_last_seen={before.get('last_seen') if before else 'N/A'}, row_after={after is not None}")
                if (
                    before is not None
                    and before.get("last_seen") is None
                    and before.get("deleted_time") is None
                    and after is not None
                ):
                    failures.append("03.3: non-tombstone row with null last_seen was not deleted by purge")
            close_db(h)

            # --- 03.4: retain tombstone row whose deleted_time is within retention window ---
            print("[03.4] purge retains tombstone rows with recent deleted_time")
            h = open_db("034.db")
            if h is not None:
                expect_ok("03.4 setup upsert", call("upsert-observed", {
                    "handle": h, "path": "b/p.txt",
                    "mod_time": OLD_TS, "byte_size": 40, "is_dir": False, "now": OLD_TS,
                }))
                expect_ok("03.4 setup tombstone", call("mark-subtree-deleted", {
                    "handle": h, "path": "b/p.txt", "deleted_time": RECENT_TS,
                }))
                before = lookup("03.4 before purge", h, "b/p.txt")
                if (
                    before is None
                    or before.get("deleted_time") != RECENT_TS
                    or before.get("last_seen") != OLD_TS
                ):
                    failures.append(
                        "03.4: precondition failed, row was not a tombstone "
                        f"with recent deleted_time and old last_seen: {before!r}"
                    )
                expect_ok("03.4 purge", call("purge-older-than", {
                    "handle": h, "retention_days": RETENTION_DAYS, "now": NOW_TS,
                }))
                after = lookup("03.4 after purge", h, "b/p.txt")
                print(
                    f"  before_deleted_time={before.get('deleted_time') if before else 'N/A'}, "
                    f"before_last_seen={before.get('last_seen') if before else 'N/A'}, "
                    f"row_after={after is not None}"
                )
                if (
                    before is not None
                    and before.get("deleted_time") == RECENT_TS
                    and before.get("last_seen") == OLD_TS
                ):
                    if after is None:
                        failures.append("03.4: recent tombstone row was incorrectly deleted by purge")
                    elif after.get("deleted_time") != RECENT_TS or after.get("last_seen") != OLD_TS:
                        failures.append(f"03.4: retained row is no longer the recent tombstone: {after!r}")
            close_db(h)

            # --- 03.5: retain non-tombstone row whose last_seen is within retention window ---
            print("[03.5] purge retains non-tombstone rows with recent last_seen")
            h = open_db("035.db")
            if h is not None:
                expect_ok("03.5 setup upsert", call("upsert-observed", {
                    "handle": h, "path": "b/q.txt",
                    "mod_time": RECENT_TS, "byte_size": 50, "is_dir": False, "now": RECENT_TS,
                }))
                before = lookup("03.5 before purge", h, "b/q.txt")
                if before is None or before.get("last_seen") != RECENT_TS or before.get("deleted_time") is not None:
                    failures.append(f"03.5: precondition failed, row was not a recent non-tombstone: {before!r}")
                expect_ok("03.5 purge", call("purge-older-than", {
                    "handle": h, "retention_days": RETENTION_DAYS, "now": NOW_TS,
                }))
                after = lookup("03.5 after purge", h, "b/q.txt")
                print(f"  before_last_seen={before.get('last_seen') if before else 'N/A'}, row_after={after is not None}")
                if before is not None and before.get("last_seen") == RECENT_TS and before.get("deleted_time") is None:
                    if after is None:
                        failures.append("03.5: recent non-tombstone row was incorrectly deleted by purge")
                    elif after.get("last_seen") != RECENT_TS or after.get("deleted_time") is not None:
                        failures.append(f"03.5: retained row is no longer the recent non-tombstone: {after!r}")
            close_db(h)

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
