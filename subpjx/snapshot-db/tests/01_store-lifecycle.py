#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Store lifecycle: open creates the DB file and snapshot table; rows persist across close/reopen."""

from __future__ import annotations

import json, os, socket, sqlite3, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "01-store-lifecycle"


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


def _call(sock, name, arguments, rpc_id):
    return _rpc(sock, "tools/call", {"name": name, "arguments": arguments}, rpc_id=rpc_id)


def main() -> int:
    TMP.mkdir(parents=True, exist_ok=True)
    db_new = TMP / "new.db"
    db_persist = TMP / "persist.db"
    for p in (db_new, db_persist):
        if p.exists():
            p.unlink()

    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = iter(range(1, 1000))

            # --- 01.1: open(file) creates a new database file at the given path ---
            r = _call(s, "open", {"file": str(db_new)}, next(rpc_id))
            handle_new = (r.get("result") or {}).get("handle")
            file_created = db_new.exists()
            print(f"[01.1] open on non-existent path creates file: {file_created}")
            if not file_created:
                failures.append("01.1: database file was not created by open()")

            _call(s, "close", {"handle": handle_new}, next(rpc_id))

            # --- 01.2: the created database contains a table named snapshot ---
            table_exists = False
            if db_new.exists():
                con = sqlite3.connect(str(db_new))
                row = con.execute(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='snapshot'"
                ).fetchone()
                con.close()
                table_exists = row is not None
            print(f"[01.2] created database contains table 'snapshot': {table_exists}")
            if not table_exists:
                failures.append("01.2: table 'snapshot' not found in created database")

            # --- 01.3: rows written before close are visible after reopen ---
            r = _call(s, "open", {"file": str(db_persist)}, next(rpc_id))
            handle_a = (r.get("result") or {}).get("handle")

            ts = "2026-05-12_00-00-00_000000Z"
            _call(s, "upsert-observed", {
                "handle": handle_a,
                "path": "a/b.txt",
                "mod_time": ts,
                "byte_size": 7,
                "is_dir": False,
                "now": ts,
            }, next(rpc_id))

            _call(s, "close", {"handle": handle_a}, next(rpc_id))

            r = _call(s, "open", {"file": str(db_persist)}, next(rpc_id))
            handle_b = (r.get("result") or {}).get("handle")

            lookup_r = _call(s, "lookup", {"handle": handle_b, "path": "a/b.txt"}, next(rpc_id))
            record = (lookup_r.get("result") or {}).get("record")
            expected = {
                "basename": "b.txt",
                "byte_size": 7,
                "deleted_time": None,
                "last_seen": ts,
                "mod_time": ts,
            }
            mismatches = []
            if record is None:
                failures.append("01.3: row not found after close and reopen of same file")
            else:
                for field, want in expected.items():
                    got = record.get(field)
                    if got != want:
                        mismatches.append(f"{field}={got!r}, want {want!r}")
                if mismatches:
                    failures.append("01.3: reopened row did not match written row: " + "; ".join(mismatches))
            print(f"[01.3] row written before close is visible after reopen: {record is not None and not mismatches}")

            _call(s, "close", {"handle": handle_b}, next(rpc_id))

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
