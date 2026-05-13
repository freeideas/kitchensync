#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""mark_subtree_deleted tombstones a path and its descendants only."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = Path(os.environ.get("AITC_PROJECT", "."))

TMP = PROJECT / "tmp" / "testks" / "03-subtree-deletion"
DB_PATH = TMP / "test.db"

OBSERVED_TS = "2026-05-12_10-00-00_000000Z"
PRE_DELETE_TS = "2026-05-12_10-01-00_000000Z"
DELETE_TS = "2026-05-12_10-02-00_000000Z"
NOOP_TS = "2026-05-12_10-03-00_000000Z"

_rpc_id = 0


def _drain(stream):
    for _ in stream:
        pass


def _launch():
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", str(PROJECT)],
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
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, arguments):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": arguments})


def _result(resp):
    return resp.get("result")


def _record(sock, handle, path):
    return (_result(_call(sock, "lookup", {"handle": handle, "path": path})) or {}).get("record")


def _delete_leftover_db():
    TMP.mkdir(parents=True, exist_ok=True)
    for path in (DB_PATH, Path(str(DB_PATH) + "-wal"), Path(str(DB_PATH) + "-shm")):
        path.unlink(missing_ok=True)


def main() -> int:
    _delete_leftover_db()

    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            handle = (_result(_call(s, "open", {"file": str(DB_PATH)})) or {}).get("handle")
            if handle is None:
                print("FATAL: open did not return a handle")
                return 1

            for path, is_dir, size in [
                ("a", True, 0),
                ("a/b", True, 0),
                ("a/b/c", False, 100),
                ("a/b/e", False, 50),
                ("a/d", False, 200),
                ("a2", False, 300),
                ("z/outside", False, 400),
            ]:
                _call(s, "upsert-observed", {
                    "handle": handle,
                    "path": path,
                    "mod_time": OBSERVED_TS,
                    "byte_size": size,
                    "is_dir": is_dir,
                    "now": OBSERVED_TS,
                })
            print("[setup] tree created")

            _call(s, "mark-subtree-deleted", {
                "handle": handle,
                "path": "a/b/e",
                "deleted_time": PRE_DELETE_TS,
            })
            pre_deleted = _record(s, handle, "a/b/e")
            pre_deleted_time = pre_deleted.get("deleted_time") if pre_deleted else None
            print(f"[setup] a/b/e.deleted_time before main call={pre_deleted_time!r}")
            if pre_deleted_time != PRE_DELETE_TS:
                failures.append(
                    f"setup: a/b/e pre-existing tombstone={pre_deleted_time!r}, "
                    f"want {PRE_DELETE_TS!r}"
                )

            _call(s, "mark-subtree-deleted", {
                "handle": handle,
                "path": "a",
                "deleted_time": DELETE_TS,
            })

            # Atomicity is not reasonably testable through this MCP wrapper: it
            # exposes one synchronous call and no hook to observe or interrupt
            # the SQL update mid-statement; testing partial failure would
            # require sabotaging the database or process.

            # --- 03.1: the requested path itself is tombstoned ---
            rec_a = _record(s, handle, "a")
            got = rec_a.get("deleted_time") if rec_a else None
            print(f"[03.1] a.deleted_time={got!r}")
            if got != DELETE_TS:
                failures.append(f"03.1: a.deleted_time={got!r}, want {DELETE_TS!r}")

            # --- 03.2: all null-tombstone transitive descendants are tombstoned ---
            for label, path in [("03.2a", "a/b"), ("03.2b", "a/b/c"), ("03.2c", "a/d")]:
                rec = _record(s, handle, path)
                got = rec.get("deleted_time") if rec else None
                print(f"[{label}] {path}.deleted_time={got!r}")
                if got != DELETE_TS:
                    failures.append(f"03.2: {path}.deleted_time={got!r}, want {DELETE_TS!r}")

            for path in ("a2", "z/outside"):
                rec = _record(s, handle, path)
                got = rec.get("deleted_time") if rec else None
                print(f"[03.2 outside] {path}.deleted_time={got!r}")
                if got is not None:
                    failures.append(f"03.2: non-descendant {path} was tombstoned: {got!r}")

            # --- 03.3: rows with a pre-existing tombstone keep that timestamp ---
            rec_e = _record(s, handle, "a/b/e")
            got_e = rec_e.get("deleted_time") if rec_e else None
            print(f"[03.3] a/b/e.deleted_time={got_e!r}")
            if got_e != PRE_DELETE_TS:
                failures.append(
                    f"03.3: a/b/e.deleted_time={got_e!r}, want original {PRE_DELETE_TS!r}"
                )

            # --- 03.4: no row at path means no rows changed and no row created ---
            observed_paths = ["a", "a/b", "a/b/c", "a/b/e", "a/d", "a2", "z/outside"]
            before_noop = {path: _record(s, handle, path) for path in observed_paths}
            _call(s, "mark-subtree-deleted", {
                "handle": handle,
                "path": "no/such/path",
                "deleted_time": NOOP_TS,
            })
            after_noop = {path: _record(s, handle, path) for path in observed_paths}
            missing_after_noop = _record(s, handle, "no/such/path")
            unchanged = before_noop == after_noop
            print(f"[03.4] existing rows unchanged after missing-path call={unchanged}")
            print(f"[03.4] lookup no/such/path={missing_after_noop!r}")
            if not unchanged:
                failures.append("03.4: existing rows changed when deleting a missing path")
            if missing_after_noop is not None:
                failures.append(f"03.4: missing path was created: {missing_after_noop!r}")

            _call(s, "close", {"handle": handle})

            if failures:
                print("\nFAILURES:")
                for failure in failures:
                    print(f"  - {failure}")
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
