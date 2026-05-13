#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises role-tag parsing rules (reqs/02_role-tags.md)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY",
                               "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_CWD = "/tmp"
_USER = "testuser"


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


def _parse(sock, text, rpc_id):
    return _rpc(sock, "tools/call",
                {"name": "parse", "arguments": {"text": text, "cwd": _CWD, "default_user": _USER}},
                rpc_id=rpc_id)


def _role(r):
    result = r.get("result") or {}
    content = result.get("content", [])
    if not content:
        return None
    parsed = json.loads(content[0].get("text", "{}"))
    return parsed.get("role")


def _is_error(r):
    return ("error" in r) or (r.get("result", {}).get("isError") is True)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 02.1 — no leading tag → role = Normal
            r = _parse(s, "file:///tmp/test", rpc_id=1)
            role = _role(r)
            print(f"[02.1] no tag → role=Normal: got {repr(role)}")
            if role != "Normal":
                failures.append(f"02.1: expected role=Normal, got {repr(role)}")

            # 02.2 — leading '+' → role = Canon
            r = _parse(s, "+file:///tmp/test", rpc_id=2)
            role = _role(r)
            print(f"[02.2] '+' tag → role=Canon: got {repr(role)}")
            if role != "Canon":
                failures.append(f"02.2: expected role=Canon, got {repr(role)}")

            # 02.3 — leading '-' → role = Subordinate
            r = _parse(s, "-file:///tmp/test", rpc_id=3)
            role = _role(r)
            print(f"[02.3] '-' tag → role=Subordinate: got {repr(role)}")
            if role != "Subordinate":
                failures.append(f"02.3: expected role=Subordinate, got {repr(role)}")

            # 02.4 — more than one role tag → rejected
            r = _parse(s, "+-file:///tmp/test", rpc_id=4)
            is_err = _is_error(r)
            print(f"[02.4] multiple role tags rejected: is_error={is_err}")
            if not is_err:
                failures.append("02.4: multiple role tags were not rejected")

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
