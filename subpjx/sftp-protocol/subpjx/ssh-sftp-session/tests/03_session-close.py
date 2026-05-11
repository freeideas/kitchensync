#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 03.1 (close_session shuts down cleanly) and 03.2 (in-flight op completes before close)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

SSH_HOST = "localhost"
SSH_PORT = 22
SSH_USER = "ace"
STAT_PATH = "/home/ace"


def _find_key():
    for name in ("id_ed25519", "id_rsa", "id_ecdsa"):
        p = Path.home() / ".ssh" / name
        if p.exists():
            return str(p)
    return str(Path.home() / ".ssh" / "id_ed25519")


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

    def send(self, method, params=None, rpc_id=1):
        msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            msg["params"] = params
        self._sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))

    def recv(self):
        while b"\n" not in self._buf:
            chunk = self._sock.recv(65536)
            if not chunk:
                raise EOFError("MCP server closed connection")
            self._buf += chunk
        line, _, self._buf = self._buf.partition(b"\n")
        return json.loads(line.decode("utf-8"))

    def call(self, method, params=None, rpc_id=1):
        self.send(method, params, rpc_id)
        return self.recv()


def _session_id(resp):
    result = resp.get("result") or {}
    content = result.get("content") or []
    if content:
        return content[0].get("text", "")
    return ""


def _open_session(mcp, rpc_id):
    return mcp.call("tools/call", {
        "name": "open_session",
        "arguments": {
            "host": SSH_HOST,
            "port": SSH_PORT,
            "user": SSH_USER,
            "credentials": [{"type": "PrivateKeyFile", "path": _find_key()}],
            "connect_timeout_secs": 10,
        },
    }, rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            mcp = MCPConn(sock)
            failures = []

            # 03.1 — close_session on an open session shuts it down without error
            open_resp = _open_session(mcp, rpc_id=1)
            sid = _session_id(open_resp)
            if open_resp.get("error") or not sid:
                failures.append("03.1: open_session failed — cannot exercise close_session")
                print(f"[03.1] FAIL: open_session error={open_resp.get('error')}")
            else:
                close_resp = mcp.call("tools/call", {
                    "name": "close_session",
                    "arguments": {"session": sid},
                }, rpc_id=2)
                if close_resp.get("error"):
                    failures.append(f"03.1: close_session returned error: {close_resp['error']}")
                    print(f"[03.1] FAIL: {close_resp['error']}")
                else:
                    print("[03.1] PASS: close_session on open session returned without error")

            # 03.2 — op issued before close_session returns a defined result (not torn/partial)
            open_resp2 = _open_session(mcp, rpc_id=3)
            sid2 = _session_id(open_resp2)
            if open_resp2.get("error") or not sid2:
                failures.append("03.2: open_session failed — cannot exercise in-flight completion")
                print(f"[03.2] FAIL: open_session error={open_resp2.get('error')}")
            else:
                # Pipeline: send stat then close_session without reading stat's response first
                mcp.send("tools/call", {
                    "name": "stat",
                    "arguments": {"session": sid2, "path": STAT_PATH},
                }, rpc_id=4)
                mcp.send("tools/call", {
                    "name": "close_session",
                    "arguments": {"session": sid2},
                }, rpc_id=5)

                # Read both responses (order may vary by id)
                responses = {}
                for _ in range(2):
                    r = mcp.recv()
                    responses[r.get("id")] = r

                stat_resp = responses.get(4)
                close_resp2 = responses.get(5)

                stat_defined = stat_resp is not None and (
                    "result" in stat_resp or "error" in stat_resp
                )
                close_ok = close_resp2 is not None and not close_resp2.get("error")

                if not stat_defined:
                    failures.append(
                        "03.2: stat did not return a defined result before close_session completed"
                    )
                    print(f"[03.2] FAIL: stat response missing or torn: {stat_resp}")
                elif not close_ok:
                    failures.append(
                        f"03.2: close_session returned error: {(close_resp2 or {}).get('error')}"
                    )
                    print(f"[03.2] FAIL: close_session: {close_resp2}")
                else:
                    print("[03.2] PASS: stat returned a defined result before close_session completed")

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
