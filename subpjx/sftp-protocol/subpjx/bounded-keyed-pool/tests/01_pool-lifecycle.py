#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises pool construction and shutdown (req 01_pool-lifecycle)."""

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


def _connect(port):
    return socket.create_connection(("127.0.0.1", port), timeout=10)


def _recv(sock, timeout=15):
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            sock.settimeout(max(0.05, deadline - time.time()))
            chunk = sock.recv(8192)
            if not chunk:
                break
            buf += chunk
            if b"\n" in buf:
                break
        except socket.timeout:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    return _recv(sock)


def _call(sock, tool, args=None, rpc_id=1):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args or {}}, rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        failures = []

        # --- 01.1: shutdown() invokes destroy on every live resource (idle and held) ---
        print("[01.1] shutdown() destroys all live resources: idle and held")
        with _connect(port) as s:
            # Reset/create pool: max 2 per key, generous idle TTL
            cp = _call(s, "create-pool", {"maxPerKey": 2, "idleTtlSeconds": 60}, rpc_id=1)
            pool_id = (cp.get("result") or {}).get("poolId")

            r1 = _call(s, "acquire", {"poolId": pool_id, "key": "k"}, rpc_id=2)
            r2 = _call(s, "acquire", {"poolId": pool_id, "key": "k"}, rpc_id=3)
            h1 = (r1.get("result") or {}).get("handleId")
            h2 = (r2.get("result") or {}).get("handleId")

            if h1 is None or h2 is None:
                failures.append(f"01.1: acquire did not return handles: r1={r1} r2={r2}")
                print(f"  FAIL: acquire returned: r1={r1} r2={r2}")
            else:
                # Release h1 → now idle. h2 remains held. Two live resources total.
                _call(s, "release", {"handleId": h1}, rpc_id=4)

                sd = _call(s, "shutdown", {"poolId": pool_id}, rpc_id=5)
                if "error" in sd:
                    failures.append(f"01.1: shutdown returned error: {sd['error']}")
                    print(f"  FAIL: shutdown error: {sd['error']}")
                else:
                    destroyed = (sd.get("result") or {}).get("destroyed_count")
                    if destroyed != 2:
                        failures.append(
                            f"01.1: expected destroyed_count=2 (1 idle + 1 held), got {destroyed}"
                        )
                        print(f"  FAIL: destroyed_count={destroyed}, expected 2")
                    else:
                        print("  PASS: destroyed_count=2 (1 idle + 1 held)")

        # --- 01.2: After shutdown(), subsequent operations are refused ---
        print("[01.2] operations after shutdown() are refused")
        with _connect(port) as s:
            cp = _call(s, "create-pool", {"maxPerKey": 2, "idleTtlSeconds": 60}, rpc_id=1)
            pool_id = (cp.get("result") or {}).get("poolId")
            _call(s, "shutdown", {"poolId": pool_id}, rpc_id=2)

            acq = _call(s, "acquire", {"poolId": pool_id, "key": "x"}, rpc_id=3)
            if "error" not in acq:
                failures.append(f"01.2: acquire after shutdown succeeded: {acq}")
                print(f"  FAIL: acquire returned: {acq}")
            else:
                print(f"  PASS: acquire refused with: {acq['error']['message']!r}")

        # --- 01.3: Acquirers blocked on acquire at shutdown time get a shutdown error ---
        print("[01.3] blocked acquirers are released with a shutdown error on shutdown()")
        blocked_result: dict = {}

        with _connect(port) as s_main:
            cp = _call(s_main, "create-pool", {"maxPerKey": 1, "idleTtlSeconds": 60}, rpc_id=1)
            pool_id = (cp.get("result") or {}).get("poolId")

            # Fill the cap for key "c" (one resource, cap=1 → next acquire blocks)
            acq1 = _call(s_main, "acquire", {"poolId": pool_id, "key": "c"}, rpc_id=2)
            h1 = (acq1.get("result") or {}).get("handleId")
            if h1 is None:
                failures.append(f"01.3: initial acquire for key 'c' failed: {acq1}")
                print(f"  FAIL: initial acquire: {acq1}")
            else:
                with _connect(port) as s_block:
                    def _blocking_acquire():
                        msg = json.dumps({
                            "jsonrpc": "2.0", "id": 99,
                            "method": "tools/call",
                            "params": {"name": "acquire", "arguments": {"poolId": pool_id, "key": "c"}},
                        }) + "\n"
                        s_block.sendall(msg.encode("utf-8"))
                        # Waits here until the server responds (blocked until shutdown)
                        blocked_result["response"] = _recv(s_block, timeout=15)

                    t = threading.Thread(target=_blocking_acquire, daemon=True)
                    t.start()

                    # Allow the acquire request to reach the server and block
                    time.sleep(0.3)

                    # Shutdown — must release the blocked acquirer with a shutdown error
                    _call(s_main, "shutdown", {"poolId": pool_id}, rpc_id=3)

                    t.join(timeout=10)

                    if t.is_alive():
                        failures.append("01.3: blocked acquire did not unblock after shutdown()")
                        print("  FAIL: blocked acquire thread still running after shutdown")
                    else:
                        resp = blocked_result.get("response", {})
                        if "error" not in resp:
                            failures.append(
                                f"01.3: blocked acquire returned success instead of shutdown error: {resp}"
                            )
                            print(f"  FAIL: blocked acquire returned: {resp}")
                        else:
                            print(f"  PASS: blocked acquire released with: {resp['error']['message']!r}")

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
