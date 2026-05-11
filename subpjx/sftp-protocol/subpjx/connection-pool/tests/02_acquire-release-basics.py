#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Verify acquire opens a connection on an empty pool and release returns it to the idle set for reuse."""

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


def _call(sock, name, arguments, rpc_id):
    return _rpc(sock, "tools/call", {"name": name, "arguments": arguments}, rpc_id=rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            tl = _rpc(s, "tools/list", rpc_id=rid); rid += 1
            tools = {t["name"]: t for t in (tl.get("result") or {}).get("tools", [])}
            print(f"[setup] tools/list: {sorted(tools.keys())}")

            # Use a unique key so this test is idempotent across reruns.
            pool_key = f"test-02-{int(time.time() * 1000)}"
            reg = _call(s, "register-pool", {"key": pool_key, "mc": 5, "ct": 5, "ka": 60}, rpc_id=rid); rid += 1
            pool = (reg.get("result") or {}).get("pool")
            print(f"[setup] register-pool key={pool_key!r} → pool={pool!r}")
            if not pool:
                failures.append("setup: register-pool did not return a pool handle")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            # --- 02.1: acquire on empty pool invokes open exactly once and returns the connection ---
            acq1 = _call(s, "acquire", {"pool": pool}, rpc_id=rid); rid += 1
            result1 = acq1.get("result") or {}
            conn1 = result1.get("connection")
            open_count_after_first = result1.get("open_count")
            print(f"[02.1] acquire → connection={conn1!r}  open_count={open_count_after_first!r}")
            if conn1:
                print("[02.1] PASS: acquire on empty pool returned a connection")
            else:
                failures.append("02.1: acquire on empty pool did not return a connection")
                print("[02.1] FAIL: no connection returned")
            if open_count_after_first is not None:
                if open_count_after_first == 1:
                    print("[02.1] PASS: open was invoked exactly once (open_count=1)")
                else:
                    failures.append(f"02.1: expected open_count=1, got {open_count_after_first}")
                    print(f"[02.1] FAIL: open_count={open_count_after_first}, want 1")

            # --- 02.2: release returns connection to idle set without invoking close immediately ---
            if conn1:
                rel = _call(s, "release", {"pool": pool, "connection": conn1}, rpc_id=rid); rid += 1
                rel_err = rel.get("error")
                rel_result = rel.get("result") or {}
                close_count_after_release = rel_result.get("close_count")
                print(f"[02.2] release → error={rel_err!r}  close_count={close_count_after_release!r}")
                if rel_err:
                    failures.append(f"02.2: release returned error: {rel_err}")
                    print(f"[02.2] FAIL: {rel_err}")
                else:
                    print("[02.2] PASS: release returned without error")
                if close_count_after_release is not None:
                    if close_count_after_release == 0:
                        print("[02.2] PASS: close was not invoked immediately (close_count=0)")
                    else:
                        failures.append(f"02.2: close invoked immediately; close_count={close_count_after_release}, want 0")
                        print(f"[02.2] FAIL: close_count={close_count_after_release}, want 0")

                # --- 02.3: subsequent acquire returns the same idle connection; open not invoked again ---
                acq2 = _call(s, "acquire", {"pool": pool}, rpc_id=rid); rid += 1
                result2 = acq2.get("result") or {}
                conn2 = result2.get("connection")
                open_count_after_second = result2.get("open_count")
                print(f"[02.3] second acquire → connection={conn2!r}  open_count={open_count_after_second!r}")
                if conn2 == conn1:
                    print("[02.3] PASS: second acquire returned the same idle connection")
                else:
                    failures.append(f"02.3: expected same connection {conn1!r}, got {conn2!r}")
                    print(f"[02.3] FAIL: got different connection; open was called again")
                if open_count_after_second is not None and open_count_after_first is not None:
                    if open_count_after_second == open_count_after_first:
                        print("[02.3] PASS: open was not invoked again (open_count unchanged)")
                    else:
                        failures.append(
                            f"02.3: open_count changed from {open_count_after_first} to {open_count_after_second}; open was called again"
                        )
                        print(f"[02.3] FAIL: open_count changed {open_count_after_first} → {open_count_after_second}")

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
