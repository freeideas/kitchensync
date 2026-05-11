#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Shutdown: close_endpoint closes idle connections, refuses acquires, lets in-flight ops complete."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TESTDIR = "/home/ace/Desktop/prjx/kitchensync/tmp/testks"


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
    deadline = time.time() + 30
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, name, args=None, rpc_id=1):
    return _rpc(sock, "tools/call", {"name": name, "arguments": args or {}}, rpc_id)


def main() -> int:
    # Ensure the SFTP list-dir target exists
    Path(TESTDIR).mkdir(parents=True, exist_ok=True)

    proc, port = _launch()
    failures = []
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            rid = iter(range(1, 1000))

            # Verify required tools are advertised
            tl = _rpc(s, "tools/list", rpc_id=next(rid))
            tools = {t["name"] for t in (tl.get("result") or {}).get("tools", [])}
            for expected in ("open-endpoint", "close-endpoint", "acquire", "release", "list-dir"):
                if expected not in tools:
                    failures.append(f"setup: tool '{expected}' missing from tools/list")
            if failures:
                print("FAILURES (setup):")
                for f in failures:
                    print(f"  - {f}")
                return 1

            # Open the endpoint
            ep_resp = _call(s, "open-endpoint", {
                "user": "ace",
                "host": "localhost",
                "settings": {"mc": 5, "ct": 30, "ka": 60},
            }, rpc_id=next(rid))
            if "error" in ep_resp:
                print(f"FATAL: open-endpoint failed: {ep_resp['error']}")
                return 1
            endpoint = ep_resp["result"]["endpoint"]
            print(f"[setup] endpoint opened: {endpoint}")

            # Acquire an in-flight connection (will not be released before close-endpoint is called)
            acq1 = _call(s, "acquire", {"endpoint": endpoint}, rpc_id=next(rid))
            if "error" in acq1:
                print(f"FATAL: first acquire failed: {acq1['error']}")
                return 1
            conn_inflight = acq1["result"]["connection"]
            print(f"[setup] in-flight connection: {conn_inflight}")

            # Acquire a second connection and release it so the pool has one idle connection
            acq2 = _call(s, "acquire", {"endpoint": endpoint}, rpc_id=next(rid))
            if "error" in acq2:
                print(f"FATAL: second acquire failed: {acq2['error']}")
                return 1
            conn_idle = acq2["result"]["connection"]
            rel2 = _call(s, "release", {"connection": conn_idle}, rpc_id=next(rid))
            if "error" in rel2:
                print(f"FATAL: release of idle connection failed: {rel2['error']}")
                return 1
            print(f"[setup] idle connection in pool: {conn_idle}")

            # 03.20: close_endpoint with one idle connection in the pool; expects success
            # (idle connection must be closed as part of pool shutdown)
            close_resp = _call(s, "close-endpoint", {"endpoint": endpoint}, rpc_id=next(rid))
            if "error" in close_resp:
                failures.append(f"03.20: close-endpoint failed: {close_resp['error']['message']}")
                print("[03.20] FAIL: close-endpoint returned an error")
            else:
                print("[03.20] close-endpoint succeeded with idle connection present in pool")

            # 03.21: acquire after close_endpoint must be refused
            acq_refused = _call(s, "acquire", {"endpoint": endpoint}, rpc_id=next(rid))
            if "error" not in acq_refused:
                failures.append("03.21: acquire after close-endpoint succeeded but must be refused")
                print("[03.21] FAIL: acquire after close-endpoint succeeded")
            else:
                print(f"[03.21] acquire correctly refused after close-endpoint")

            # 03.22: in-flight operation (on the not-yet-released connection) must complete
            op_resp = _call(s, "list-dir", {"connection": conn_inflight, "path": TESTDIR}, rpc_id=next(rid))
            if "error" in op_resp:
                failures.append(f"03.22: in-flight list-dir must complete after close-endpoint but got error: {op_resp['error']['message']}")
                print(f"[03.22] FAIL: in-flight list-dir failed: {op_resp['error']}")
            else:
                entries = op_resp["result"].get("entries", [])
                print(f"[03.22] in-flight list-dir completed after close-endpoint ({len(entries)} entries)")

            # 03.23: releasing the in-flight connection closes it rather than returning it to the pool;
            # verified by: release succeeds, then acquire is still refused (pool has no revived connection)
            rel_inflight = _call(s, "release", {"connection": conn_inflight}, rpc_id=next(rid))
            if "error" in rel_inflight:
                failures.append(f"03.23: release of in-flight connection after close-endpoint failed: {rel_inflight['error']['message']}")
                print("[03.23] FAIL: release of in-flight connection failed")
            else:
                acq_still_refused = _call(s, "acquire", {"endpoint": endpoint}, rpc_id=next(rid))
                if "error" not in acq_still_refused:
                    failures.append("03.23: acquire succeeded after releasing in-flight connection — connection was returned to pool instead of closed")
                    print("[03.23] FAIL: acquire succeeded after in-flight release — connection not closed")
                else:
                    print("[03.23] in-flight connection correctly closed after release (acquire still refused)")

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
