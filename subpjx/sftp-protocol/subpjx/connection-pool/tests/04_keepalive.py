#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Keepalive: idle connections reused within ka window, closed after ka expires, timer cancelled on reacquire."""

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
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # --- 04.1: connection released to a live pool is reusable within ka seconds ---
            print("[04.1] registering pool-04-1 (ka=2)")
            r = _call(s, "register-pool", {"key": "pool-04-1", "mc": 1, "ct": 5, "ka": 2}, rid); rid += 1
            pool1 = (r.get("result") or {}).get("pool_id")

            r = _call(s, "acquire", {"pool_id": pool1}, rid); rid += 1
            conn1 = (r.get("result") or {}).get("connection_id")

            _call(s, "release", {"pool_id": pool1, "connection_id": conn1}, rid); rid += 1

            time.sleep(0.5)  # within ka=2 window

            r = _call(s, "acquire", {"pool_id": pool1}, rid); rid += 1
            conn1b = (r.get("result") or {}).get("connection_id")

            r = _call(s, "get-open-count", {"pool_id": pool1}, rid); rid += 1
            open_count1 = (r.get("result") or {}).get("count", -1)

            print(f"[04.1] open_count={open_count1}, conn1={conn1}, reacquired={conn1b}")
            if open_count1 != 1:
                failures.append(f"04.1: expected open called once for reuse within ka, got {open_count1}")
            if conn1b != conn1:
                failures.append(f"04.1: expected same connection_id on idle reuse, got {conn1b!r} != {conn1!r}")
            if open_count1 == 1 and conn1b == conn1:
                print("[04.1] PASS: released connection reused without invoking open")

            # --- 04.2: close invoked on idle connection after ka elapses ---
            print("[04.2] registering pool-04-2 (ka=1)")
            r = _call(s, "register-pool", {"key": "pool-04-2", "mc": 1, "ct": 5, "ka": 1}, rid); rid += 1
            pool2 = (r.get("result") or {}).get("pool_id")

            r = _call(s, "acquire", {"pool_id": pool2}, rid); rid += 1
            conn2 = (r.get("result") or {}).get("connection_id")

            _call(s, "release", {"pool_id": pool2, "connection_id": conn2}, rid); rid += 1

            time.sleep(2.0)  # past ka=1 window

            r = _call(s, "get-close-count", {"pool_id": pool2}, rid); rid += 1
            close_count2 = (r.get("result") or {}).get("count", -1)

            print(f"[04.2] close_count={close_count2}")
            if close_count2 != 1:
                failures.append(f"04.2: expected close called once after ka elapsed, got {close_count2}")
            else:
                print("[04.2] PASS: pool invoked close on idle connection after ka expired")

            # --- 04.3: reacquiring an idle connection cancels its ka timer ---
            print("[04.3] registering pool-04-3 (ka=2)")
            r = _call(s, "register-pool", {"key": "pool-04-3", "mc": 1, "ct": 5, "ka": 2}, rid); rid += 1
            pool3 = (r.get("result") or {}).get("pool_id")

            r = _call(s, "acquire", {"pool_id": pool3}, rid); rid += 1
            conn3 = (r.get("result") or {}).get("connection_id")

            _call(s, "release", {"pool_id": pool3, "connection_id": conn3}, rid); rid += 1

            time.sleep(0.5)  # within ka=2 window; timer is running

            # reacquire — must cancel the old ka timer
            r = _call(s, "acquire", {"pool_id": pool3}, rid); rid += 1
            conn3b = (r.get("result") or {}).get("connection_id")

            # release immediately so the connection is idle when the old timer would have fired;
            # this starts a fresh ka=2 timer, which fires ~2s from now (well after the check below).
            # old timer was set at t≈0 and would fire at t≈2.0.
            # new timer fires at t≈2.5 (0.5s reacquire + ~0s release + 2.0s ka).
            # check at t≈2.2: old timer window elapsed, connection is idle, new timer has not yet fired.
            _call(s, "release", {"pool_id": pool3, "connection_id": conn3b}, rid); rid += 1

            time.sleep(1.7)  # advance to ~2.2s after old release; old timer would have fired; new timer has not

            r = _call(s, "get-close-count", {"pool_id": pool3}, rid); rid += 1
            close_count3 = (r.get("result") or {}).get("count", -1)

            print(f"[04.3] close_count={close_count3}, conn3={conn3}, reacquired={conn3b}")
            if conn3b != conn3:
                failures.append(f"04.3: expected same connection_id on idle reuse, got {conn3b!r} != {conn3!r}")
            if close_count3 != 0:
                failures.append(f"04.3: expected ka timer cancelled on reacquire (close_count=0), got {close_count3}")
            if conn3b == conn3 and close_count3 == 0:
                print("[04.3] PASS: ka timer cancelled on reacquire; connection not subsequently closed")

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
