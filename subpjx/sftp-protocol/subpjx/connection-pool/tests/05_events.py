#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 05.1–05.5: on_event invoked once per acquire/release with correct field values."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

MC = 3
KEY = "host-05-events"


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


class MCPConn:
    def __init__(self, sock):
        self._sock = sock
        self._buf = b""
        self._sock.settimeout(15)

    def call(self, method, params=None, rpc_id=1):
        msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            msg["params"] = params
        self._sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
        while b"\n" not in self._buf:
            chunk = self._sock.recv(65536)
            if not chunk:
                raise EOFError("MCP server closed connection")
            self._buf += chunk
        line, _, self._buf = self._buf.partition(b"\n")
        return json.loads(line.decode("utf-8"))


def _tool(mcp, name, args, rpc_id):
    return mcp.call("tools/call", {"name": name, "arguments": args}, rpc_id)


def _text(resp):
    result = resp.get("result") or {}
    content = result.get("content") or []
    return content[0].get("text", "") if content else ""


def _events(mcp, pool_id, rpc_id):
    resp = _tool(mcp, "get_events", {"pool": pool_id}, rpc_id)
    t = _text(resp)
    if not t:
        return []
    try:
        return json.loads(t)
    except (json.JSONDecodeError, ValueError):
        return []


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            mcp = MCPConn(sock)
            failures = []
            rid = 1

            # Register pool with on_event enabled
            reg = _tool(mcp, "register_pool", {
                "key": KEY, "mc": MC, "ct": 30, "ka": 60, "on_event": True,
            }, rid); rid += 1
            if reg.get("error"):
                print(f"[setup] FAIL: register_pool error: {reg['error']}")
                failures.append("setup: register_pool failed")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            pool = _text(reg)
            print(f"[setup] pool={pool!r}")

            # Acquire first connection
            acq1 = _tool(mcp, "acquire", {"pool": pool}, rid); rid += 1
            conn1 = _text(acq1)
            evts_post_acq1 = _events(mcp, pool, rid); rid += 1
            acq_evts = [e for e in evts_post_acq1 if e.get("kind") == "acquire"]

            # 05.1 — acquire event carries kind="acquire", key, in_use, mc; fired exactly once
            if len(acq_evts) != 1:
                failures.append(f"05.1: expected exactly 1 acquire event, got {len(acq_evts)}")
                print(f"[05.1] FAIL: expected 1 acquire event, got {len(acq_evts)}")
            else:
                e = acq_evts[0]
                if (e.get("kind") == "acquire"
                        and e.get("key") == KEY
                        and isinstance(e.get("in_use"), int)
                        and e.get("mc") == MC):
                    print(f"[05.1] PASS: acquire event={e}")
                else:
                    failures.append(f"05.1: acquire event fields wrong: {e}")
                    print(f"[05.1] FAIL: acquire event={e}")

            # 05.3 — acquire in_use is one greater than pre-acquire count (0 → 1)
            if not acq_evts:
                failures.append("05.3: no acquire event to check in_use")
                print("[05.3] FAIL: no acquire event")
            else:
                e = acq_evts[-1]
                if e.get("in_use") == 1:
                    print(f"[05.3] PASS: first acquire in_use=1 (pre-acquire count was 0)")
                else:
                    failures.append(f"05.3: first acquire in_use={e.get('in_use')}, expected 1")
                    print(f"[05.3] FAIL: expected in_use=1, got {e.get('in_use')}")

            # Release conn1
            if conn1 and not acq1.get("error"):
                rel1 = _tool(mcp, "release", {"pool": pool, "connection": conn1}, rid); rid += 1
                evts_post_rel1 = _events(mcp, pool, rid); rid += 1
                rel_evts = [e for e in evts_post_rel1 if e.get("kind") == "release"]
            else:
                rel_evts = []

            # 05.2 — release event carries kind="release", key, in_use, mc; fired exactly once
            if len(rel_evts) != 1:
                failures.append(f"05.2: expected exactly 1 release event, got {len(rel_evts)}")
                print(f"[05.2] FAIL: expected 1 release event, got {len(rel_evts)}")
            else:
                e = rel_evts[0]
                if (e.get("kind") == "release"
                        and e.get("key") == KEY
                        and isinstance(e.get("in_use"), int)
                        and e.get("mc") == MC):
                    print(f"[05.2] PASS: release event={e}")
                else:
                    failures.append(f"05.2: release event fields wrong: {e}")
                    print(f"[05.2] FAIL: release event={e}")

            # 05.4 — release in_use is one less than pre-release count (1 → 0)
            if not rel_evts:
                failures.append("05.4: no release event to check in_use")
                print("[05.4] FAIL: no release event")
            else:
                e = rel_evts[-1]
                if e.get("in_use") == 0:
                    print(f"[05.4] PASS: first release in_use=0 (pre-release count was 1)")
                else:
                    failures.append(f"05.4: first release in_use={e.get('in_use')}, expected 0")
                    print(f"[05.4] FAIL: expected in_use=0, got {e.get('in_use')}")

            _tool(mcp, "close_pool", {"pool": pool}, rid); rid += 1

            # 05.5 — on_event=none: no events on acquire or release
            KEY2 = "host-05-noevt"
            reg2 = _tool(mcp, "register_pool", {
                "key": KEY2, "mc": 2, "ct": 30, "ka": 60, "on_event": False,
            }, rid); rid += 1
            if reg2.get("error"):
                failures.append(f"05.5: register_pool (on_event=false) failed: {reg2['error']}")
                print(f"[05.5] FAIL: register_pool: {reg2['error']}")
            else:
                pool2 = _text(reg2)
                acq2 = _tool(mcp, "acquire", {"pool": pool2}, rid); rid += 1
                conn2 = _text(acq2)
                if conn2 and not acq2.get("error"):
                    _tool(mcp, "release", {"pool": pool2, "connection": conn2}, rid); rid += 1
                evts5 = _events(mcp, pool2, rid); rid += 1
                if evts5 == []:
                    print("[05.5] PASS: no events recorded when on_event=none")
                else:
                    failures.append(f"05.5: expected no events but got {evts5}")
                    print(f"[05.5] FAIL: got events={evts5}")
                _tool(mcp, "close_pool", {"pool": pool2}, rid); rid += 1

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
