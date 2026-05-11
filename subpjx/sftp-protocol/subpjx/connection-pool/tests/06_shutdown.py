#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 06_shutdown: close_pool closes idle connections, refuses new acquires, and defers in-use closes to release."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


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


def _call(sock, tool, args, rpc_id):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # --- 06.1: close_pool invokes close on every idle connection ---
            # Register a pool, acquire a connection, release it (making it idle),
            # then shut down the pool and confirm close was called on the idle connection.
            r = _call(s, "register-pool", {"key": "06-1", "mc": 2, "ct": 5, "ka": 60}, rid); rid += 1
            pool_1 = (r.get("result") or {}).get("pool")

            r = _call(s, "acquire", {"pool": pool_1}, rid); rid += 1
            conn_1 = (r.get("result") or {}).get("connection")

            _call(s, "release", {"pool": pool_1, "connection": conn_1}, rid); rid += 1

            r_before = _call(s, "get-close-count", {"pool": pool_1}, rid); rid += 1
            count_before_1 = (r_before.get("result") or {}).get("count", 0)

            _call(s, "close-pool", {"pool": pool_1}, rid); rid += 1

            r_after = _call(s, "get-close-count", {"pool": pool_1}, rid); rid += 1
            count_after_1 = (r_after.get("result") or {}).get("count", 0)

            print(f"[06.1] close count before close_pool={count_before_1}, after={count_after_1}")
            if count_after_1 == count_before_1 + 1:
                print("[06.1] PASS: close was invoked on the idle connection at shutdown")
            else:
                print("[06.1] FAIL: expected close count to increase by 1 for idle connection")
                failures.append("06.1: close not invoked on idle connection at shutdown")

            # --- 06.2: acquire fails after close_pool ---
            r = _call(s, "register-pool", {"key": "06-2", "mc": 2, "ct": 5, "ka": 60}, rid); rid += 1
            pool_2 = (r.get("result") or {}).get("pool")

            _call(s, "close-pool", {"pool": pool_2}, rid); rid += 1

            r = _call(s, "acquire", {"pool": pool_2}, rid); rid += 1
            print(f"[06.2] acquire on shut-down pool: {'error' if 'error' in r else 'result=' + str(r.get('result'))}")
            if "error" in r:
                print("[06.2] PASS: acquire failed on shut-down pool")
            else:
                print("[06.2] FAIL: acquire should have failed after close_pool")
                failures.append("06.2: acquire succeeded on shut-down pool")

            # --- 06.3 and 06.4: in-use connections at shutdown are not interrupted;
            #     they are closed (not re-pooled) when later released ---
            r = _call(s, "register-pool", {"key": "06-3-4", "mc": 2, "ct": 5, "ka": 60}, rid); rid += 1
            pool_34 = (r.get("result") or {}).get("pool")

            r = _call(s, "acquire", {"pool": pool_34}, rid); rid += 1
            conn_34 = (r.get("result") or {}).get("connection")

            r_before_cp = _call(s, "get-close-count", {"pool": pool_34}, rid); rid += 1
            count_before_cp = (r_before_cp.get("result") or {}).get("count", 0)

            _call(s, "close-pool", {"pool": pool_34}, rid); rid += 1

            # 06.3: in-use connection must not be closed by close_pool itself
            r_after_cp = _call(s, "get-close-count", {"pool": pool_34}, rid); rid += 1
            count_after_cp = (r_after_cp.get("result") or {}).get("count", 0)

            print(f"[06.3] close count after close_pool: {count_after_cp} (expected {count_before_cp}, no change)")
            if count_after_cp == count_before_cp:
                print("[06.3] PASS: in-use connection was not closed when close_pool was called")
            else:
                print("[06.3] FAIL: in-use connection was closed by close_pool")
                failures.append("06.3: in-use connection was interrupted by close_pool")

            # 06.4: releasing the in-use connection after shutdown must close it (not re-pool it)
            r_rel = _call(s, "release", {"pool": pool_34, "connection": conn_34}, rid); rid += 1
            print(f"[06.4] release after shutdown: {'ok' if 'error' not in r_rel else 'error=' + str(r_rel.get('error'))}")

            r_after_rel = _call(s, "get-close-count", {"pool": pool_34}, rid); rid += 1
            count_after_rel = (r_after_rel.get("result") or {}).get("count", 0)

            print(f"[06.4] close count after release: {count_after_rel} (expected {count_after_cp + 1})")
            if "error" not in r_rel and count_after_rel == count_after_cp + 1:
                print("[06.4] PASS: connection was closed on release after shutdown")
            else:
                print("[06.4] FAIL: connection was not closed when released after shutdown")
                failures.append("06.4: in-use connection not closed on release after shutdown")

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
